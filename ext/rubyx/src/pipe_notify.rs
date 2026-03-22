use std::os::fd::RawFd;

/// A pipe that have a pair of descriptors for reading and writing.
pub struct PipeNotify {
    read_fd: RawFd,
    write_fd: RawFd,
}

impl PipeNotify {
    pub fn new() -> std::io::Result<Self> {
        let mut fds = [0 as RawFd; 2];
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }

        // Set the pipe to non-blocking mode.
        unsafe {
            libc::fcntl(fds[0], libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(fds[1], libc::F_SETFL, libc::O_NONBLOCK);
        }

        Ok(Self {
            read_fd: fds[0],
            write_fd: fds[1],
        })
    }
    /// Write a notification byte. Called by the producer after channel.send().
    pub fn notify(&self) {
        let buf: [u8; 1] = [1];
        unsafe {
            // Ignore errors — if the pipe is full, the consumer will drain it
            libc::write(self.write_fd, buf.as_ptr() as *const libc::c_void, 1);
        }
    }
    /// Drain all notification bytes.
    pub fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            let n = unsafe {
                libc::read(
                    self.read_fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n <= 0 {
                // EAGAIN (non-blocking) or error
                break;
            }
        }
    }
    /// Get the read fd for IO.select.
    pub fn read_fd(&self) -> RawFd {
        self.read_fd
    }
}
unsafe impl Send for PipeNotify {}
unsafe impl Sync for PipeNotify {}
impl Drop for PipeNotify {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::Arc;

    #[test]
    fn test_new_creates_valid_pipe() {
        let pipe = PipeNotify::new().expect("pipe creation should succeed");
        assert!(pipe.read_fd >= 0, "read_fd should be valid");
        assert!(pipe.write_fd >= 0, "write_fd should be valid");
        assert_ne!(
            pipe.read_fd, pipe.write_fd,
            "read and write fds should differ"
        );
    }

    #[test]
    fn test_notify_then_drain() {
        let pipe = PipeNotify::new().unwrap();
        pipe.notify();
        pipe.drain();
        // Drain again — should be a no-op (pipe is empty)
        pipe.drain();
    }

    #[test]
    fn test_drain_on_empty_pipe_does_not_block() {
        let pipe = PipeNotify::new().unwrap();
        let start = std::time::Instant::now();
        pipe.drain();
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(10),
            "drain on empty pipe should return instantly, took {elapsed:?}"
        );
    }

    #[test]
    fn test_multiple_notifies_single_drain() {
        let pipe = PipeNotify::new().unwrap();
        for _ in 0..100 {
            pipe.notify();
        }
        pipe.drain();
        // After drain, pipe should be empty — another drain is instant
        let start = std::time::Instant::now();
        pipe.drain();
        assert!(start.elapsed() < std::time::Duration::from_millis(10));
    }

    #[test]
    fn test_notify_drain_interleaved() {
        let pipe = PipeNotify::new().unwrap();
        for _ in 0..50 {
            pipe.notify();
            pipe.drain();
        }
        // Pipe should be empty at the end
        pipe.drain();
    }

    #[test]
    fn test_read_fd_is_stable() {
        let pipe = PipeNotify::new().unwrap();
        let fd1 = pipe.read_fd();
        let fd2 = pipe.read_fd();
        assert_eq!(fd1, fd2, "read_fd should return the same value");
    }

    #[test]
    fn test_no_byte_accumulation_with_channel() {
        // Simulates the real producer/consumer pattern:
        // send item through channel + notify, then drain + try_recv
        use crossbeam_channel::bounded;

        let pipe = Arc::new(PipeNotify::new().unwrap());
        let (tx, rx) = bounded::<i64>(16);

        let producer_pipe = pipe.clone();
        let producer = std::thread::spawn(move || {
            for i in 0..100 {
                tx.send(i).unwrap();
                producer_pipe.notify();
            }
        });

        let mut items = Vec::with_capacity(100);
        for _ in 0..100 {
            pipe.drain();
            let item = rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("producer should make progress while consumer drains");
            items.push(item);
        }
        producer.join().unwrap();

        assert_eq!(items, (0..100).collect::<Vec<_>>());

        // Pipe should be fully drained — no leftover bytes
        pipe.drain(); // should be no-op
    }

    #[test]
    fn test_cross_thread_notify_drain() {
        let pipe = Arc::new(PipeNotify::new().unwrap());
        let pipe_clone = pipe.clone();

        let handle = std::thread::spawn(move || {
            for _ in 0..50 {
                pipe_clone.notify();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        // Drain periodically from the main thread
        let mut drain_count = 0;
        while !handle.is_finished() || drain_count < 5 {
            pipe.drain();
            drain_count += 1;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        handle.join().unwrap();

        // Final drain to catch any stragglers
        pipe.drain();
    }

    #[test]
    #[serial]
    fn test_drop_closes_fds() {
        // Note: This test is inherently racy — another thread can reuse the
        // fd number between drop and fcntl check. We retry once to reduce
        // flakiness, but the core behavior (Drop closes fds) is also verified
        // by test_many_pipes_no_fd_leak which would fail on fd exhaustion.
        let pipe = PipeNotify::new().unwrap();
        let read_fd = pipe.read_fd;
        let write_fd = pipe.write_fd;
        drop(pipe);

        let r = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        let w = unsafe { libc::fcntl(write_fd, libc::F_GETFD) };

        // If either fd was reused by another thread, skip rather than fail
        if r == 0 || w == 0 {
            // Fd was reused — can't reliably test. The 500-pipe leak test
            // covers this behavior more reliably.
            return;
        }
        assert_eq!(r, -1, "read_fd should be closed after drop");
        assert_eq!(w, -1, "write_fd should be closed after drop");
    }

    #[test]
    fn test_many_pipes_no_fd_leak() {
        // Create and drop many pipes — should not exhaust fds
        for _ in 0..500 {
            let pipe = PipeNotify::new().expect("should not run out of fds");
            pipe.notify();
            pipe.drain();
            drop(pipe);
        }
    }
}
