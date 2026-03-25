use crate::gvl::{rb_thread_call_without_gvl, rb_thread_check_ints, recv_loop, ubf_cancel};
use crate::pipe_notify::PipeNotify;
use crate::ruby_helpers::runtime_error;
use crate::stream::StreamItem;
use crossbeam_channel::Receiver;
use magnus::value::ReprValue;
use magnus::{Error, Module, Ruby, Value};
use std::ffi::c_void;
use std::os::fd::RawFd;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn create_ruby_io(ruby: &Ruby, fd: RawFd) -> Result<Value, Error> {
    let io_class: Value = ruby.class_object().const_get("IO")?;
    io_class.funcall("for_fd", (fd, magnus::kwargs!("autoclose" => false)))
}

fn has_fiber_scheduler(ruby: &Ruby) -> bool {
    let fiber_class = ruby
        .class_object()
        .const_get("Fiber")
        .unwrap_or_else(|_| ruby.qnil().as_value());
    let scheduler = fiber_class
        .funcall("scheduler", ())
        .unwrap_or_else(|_| ruby.qnil().as_value());
    !scheduler.is_nil()
}
pub(crate) unsafe extern "C" fn recv_without_gvl_cb(args: *mut c_void) -> *mut c_void {
    let args = &mut *(args as *mut crate::nonblocking_stream::RecvArgs);
    args.result = recv_loop(&args.receiver, &args.cancel);
    std::ptr::null_mut()
}
struct RecvArgs {
    receiver: Receiver<StreamItem>,
    result: Option<Result<StreamItem, crossbeam_channel::RecvError>>,
    cancel: Arc<AtomicBool>,
}
#[magnus::wrap(class = "Rubyx::NonBlockingStream", free_immediately)]
pub(crate) struct NonBlockingStream {
    receiver: Receiver<StreamItem>,
    pipe: Arc<PipeNotify>,
    #[allow(dead_code)]
    cancel: Arc<AtomicBool>,
}
impl NonBlockingStream {
    pub(crate) fn new(receiver: Receiver<StreamItem>, pipe: Arc<PipeNotify>) -> Self {
        NonBlockingStream {
            receiver,
            pipe,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn next_gvl_release(&self) -> Option<Result<Value, Error>> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = RecvArgs {
            receiver: self.receiver.clone(),
            result: None,
            cancel: cancel.clone(),
        };

        unsafe {
            rb_thread_call_without_gvl(
                recv_without_gvl_cb,
                &mut args as *mut RecvArgs as *mut c_void,
                Some(ubf_cancel),
                Arc::as_ptr(&cancel) as *mut c_void,
            );
            rb_thread_check_ints();
        }
        match args.result? {
            Ok(StreamItem::Value(v)) => Some(v.try_into()),
            Ok(StreamItem::Error(e)) => Some(Err(Error::new(runtime_error(), e))),
            Ok(StreamItem::End) | Err(_) => None,
        }
    }

    fn each_fiber_aware(&self, ruby: &Ruby) -> Result<(), Error> {
        let read_io = create_ruby_io(ruby, self.pipe.read_fd())?;
        let select_arr = ruby.ary_new_from_values(&[read_io]);
        loop {
            // IO.select([read_io])
            let io = ruby
                .class_object()
                .const_get("IO")
                .unwrap_or_else(|_| ruby.qnil().as_value());
            let nil = ruby.qnil().as_value();
            let _: Value = io.funcall("select", (select_arr, nil, nil))?;

            self.pipe.drain();
            loop {
                match self.receiver.try_recv() {
                    Ok(StreamItem::Value(v)) => {
                        let val: Value = v.try_into()?;
                        let _: Value = ruby.yield_value(val)?;
                    }
                    Ok(StreamItem::Error(e)) => {
                        return Err(Error::new(ruby.exception_runtime_error(), e));
                    }
                    Ok(StreamItem::End) => return Ok(()),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
                }
            }
        }
    }

    pub fn each(&self) -> Result<(), Error> {
        let ruby = Ruby::get().expect("called from Ruby thread");
        if has_fiber_scheduler(&ruby) {
            self.each_fiber_aware(&ruby)
        } else {
            loop {
                match self.next_gvl_release() {
                    Some(Ok(value)) => {
                        let _: Value = ruby.yield_value(value)?;
                    }
                    Some(Err(e)) => return Err(e),
                    None => return Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::SendableValue;
    use crate::test_helpers::with_ruby_python;
    use crossbeam_channel::{bounded, unbounded};
    use magnus::value::ReprValue;
    use magnus::RArray;
    use magnus::TryConvert;
    use serial_test::serial;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    // ========== Step 3: recv_without_gvl_cb (pure Rust, no Ruby needed) ==========

    #[test]
    fn test_recv_cb_delivers_value() {
        let (tx, rx) = unbounded();
        tx.send(StreamItem::Value(SendableValue::Integer(42)))
            .unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }

        match args.result.unwrap().unwrap() {
            StreamItem::Value(SendableValue::Integer(n)) => assert_eq!(n, 42),
            _ => panic!("expected Integer(42)"),
        }
    }

    #[test]
    fn test_recv_cb_delivers_end() {
        let (tx, rx) = unbounded();
        tx.send(StreamItem::End).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }

        assert!(matches!(args.result.unwrap().unwrap(), StreamItem::End));
    }

    #[test]
    fn test_recv_cb_delivers_error() {
        let (tx, rx) = unbounded();
        tx.send(StreamItem::Error("boom".to_string())).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }

        match args.result.unwrap().unwrap() {
            StreamItem::Error(msg) => assert_eq!(msg, "boom"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_recv_cb_handles_disconnect() {
        let (tx, rx) = bounded::<StreamItem>(16);
        drop(tx);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }

        assert!(args.result.unwrap().is_err());
    }

    #[test]
    fn test_recv_cb_multiple_values_in_order() {
        let (tx, rx) = unbounded();
        for i in 0..5 {
            tx.send(StreamItem::Value(SendableValue::Integer(i)))
                .unwrap();
        }
        tx.send(StreamItem::End).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));

        for expected in 0..5 {
            let mut args = RecvArgs {
                receiver: rx.clone(),
                result: None,
                cancel: cancel.clone(),
            };
            unsafe {
                recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
            }
            match args.result.unwrap().unwrap() {
                StreamItem::Value(SendableValue::Integer(n)) => assert_eq!(n, expected),
                _ => panic!("expected Integer({expected})"),
            }
        }

        // Next should be End
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };
        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }
        assert!(matches!(args.result.unwrap().unwrap(), StreamItem::End));
    }

    // ========== Step 4: ubf_cancel and cancel flag ==========

    #[test]
    fn test_ubf_cancel_sets_flag() {
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(!cancel.load(Ordering::Relaxed));

        unsafe {
            ubf_cancel(Arc::as_ptr(&cancel) as *mut c_void);
        }

        assert!(cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn test_recv_cb_respects_cancel_flag() {
        // Empty channel — recv would block forever without cancel
        let (_tx, rx) = bounded::<StreamItem>(16);

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();

        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        // Set cancel from another thread after a short delay
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancel_clone.store(true, Ordering::Relaxed);
        });

        let start = Instant::now();
        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }
        let elapsed = start.elapsed();

        // Should return None (cancelled), not block forever
        assert!(args.result.is_none(), "expected None on cancel");
        // Should return within ~100ms (50ms timeout + margin)
        assert!(
            elapsed < Duration::from_millis(200),
            "cancel took {elapsed:?}, expected < 200ms"
        );
    }

    #[test]
    fn test_recv_cb_cancel_flag_already_set() {
        // Cancel flag set before calling — should return immediately
        let (_tx, rx) = bounded::<StreamItem>(16);

        let cancel = Arc::new(AtomicBool::new(true)); // pre-set
        let mut args = RecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        let start = Instant::now();
        unsafe {
            recv_without_gvl_cb(&mut args as *mut RecvArgs as *mut c_void);
        }
        let elapsed = start.elapsed();

        assert!(args.result.is_none(), "expected None on pre-set cancel");
        assert!(
            elapsed < Duration::from_millis(10),
            "pre-set cancel took {elapsed:?}, expected near-instant"
        );
    }

    // ========== Helper: construct NonBlockingStream for tests ==========

    fn make_stream(rx: Receiver<StreamItem>) -> NonBlockingStream {
        let pipe = Arc::new(PipeNotify::new().unwrap());
        NonBlockingStream {
            receiver: rx,
            pipe,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
    fn make_stream_with_pipe(rx: Receiver<StreamItem>, pipe: Arc<PipeNotify>) -> NonBlockingStream {
        NonBlockingStream {
            receiver: rx,
            pipe,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    // ========== Step 3+4: next_gvl_release integration (needs Ruby) ==========

    #[test]
    #[serial]
    fn test_next_gvl_release_delivers_value() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();
            tx.send(StreamItem::Value(SendableValue::Integer(42)))
                .unwrap();

            let stream = make_stream(rx);
            let val = stream.next_gvl_release().unwrap().unwrap();
            assert_eq!(i64::try_convert(val).unwrap(), 42);
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_multiple_values_in_order() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();
            for i in 0..5 {
                tx.send(StreamItem::Value(SendableValue::Integer(i)))
                    .unwrap();
            }
            tx.send(StreamItem::End).unwrap();

            let stream = make_stream(rx);
            for expected in 0..5 {
                let val = stream.next_gvl_release().unwrap().unwrap();
                assert_eq!(i64::try_convert(val).unwrap(), expected);
            }
            assert!(stream.next_gvl_release().is_none());
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_returns_none_on_end() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();
            tx.send(StreamItem::End).unwrap();

            let stream = make_stream(rx);
            assert!(stream.next_gvl_release().is_none());
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_returns_none_on_disconnect() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = bounded::<StreamItem>(16);
            drop(tx);

            let stream = make_stream(rx);
            assert!(stream.next_gvl_release().is_none());
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_propagates_error() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();
            tx.send(StreamItem::Error("something broke".to_string()))
                .unwrap();

            let stream = make_stream(rx);
            let err = stream.next_gvl_release().unwrap().unwrap_err();
            assert!(err.to_string().contains("something broke"));
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_all_sendable_types() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();
            tx.send(StreamItem::Value(SendableValue::Nil)).unwrap();
            tx.send(StreamItem::Value(SendableValue::Integer(99)))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Float(2.5)))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Str("hello".to_string())))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Bool(true)))
                .unwrap();
            tx.send(StreamItem::End).unwrap();

            let stream = make_stream(rx);

            let v = stream.next_gvl_release().unwrap().unwrap();
            assert!(v.is_nil());

            let v = stream.next_gvl_release().unwrap().unwrap();
            assert_eq!(i64::try_convert(v).unwrap(), 99);

            let v = stream.next_gvl_release().unwrap().unwrap();
            assert!((f64::try_convert(v).unwrap() - 2.5).abs() < 1e-9);

            let v = stream.next_gvl_release().unwrap().unwrap();
            assert_eq!(String::try_convert(v).unwrap(), "hello");

            let v = stream.next_gvl_release().unwrap().unwrap();
            assert!(bool::try_convert(v).unwrap());

            assert!(stream.next_gvl_release().is_none());
        });
    }

    #[test]
    #[serial]
    fn test_next_gvl_release_with_delayed_producer() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = unbounded();

            // Producer sends after a delay — recv must wait without deadlocking
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                tx.send(StreamItem::Value(SendableValue::Integer(7)))
                    .unwrap();
                tx.send(StreamItem::End).unwrap();
            });

            let stream = make_stream(rx);

            let start = Instant::now();
            let val = stream.next_gvl_release().unwrap().unwrap();
            let elapsed = start.elapsed();

            assert_eq!(i64::try_convert(val).unwrap(), 7);
            assert!(
                elapsed >= Duration::from_millis(50),
                "should have waited for producer, took {elapsed:?}"
            );
            assert!(stream.next_gvl_release().is_none());
        });
    }

    // ========== Step 6: create_ruby_io ==========

    #[test]
    #[serial]
    fn test_create_ruby_io_returns_io_object() {
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();

            // Verify it's an instance of IO
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();
            let is_io: bool = io.funcall("is_a?", (io_class,)).unwrap();
            assert!(is_io, "create_ruby_io should return an IO instance");
        });
    }

    #[test]
    #[serial]
    fn test_create_ruby_io_fileno_matches_pipe_fd() {
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let expected_fd = pipe.read_fd();
            let io = create_ruby_io(ruby, expected_fd).unwrap();

            let fileno: i32 = io.funcall("fileno", ()).unwrap();
            assert_eq!(
                fileno, expected_fd,
                "IO#fileno should match the pipe's read_fd"
            );
        });
    }

    #[test]
    #[serial]
    fn test_create_ruby_io_autoclose_is_false() {
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();

            let autoclose: bool = io.funcall("autoclose?", ()).unwrap();
            assert!(
                !autoclose,
                "autoclose must be false to prevent double-close with PipeNotify::drop"
            );
        });
    }

    #[test]
    #[serial]
    fn test_create_ruby_io_fd_survives_gc() {
        // Verifies that Ruby GC does NOT close the fd because autoclose: false.
        // After GC, the fd should still be valid (PipeNotify owns it).
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let fd = pipe.read_fd();

            {
                let _io = create_ruby_io(ruby, fd).unwrap();
                // io goes out of Rust scope here — Ruby may GC it
            }

            // Force Ruby GC
            let _: Value = ruby.eval("GC.start").unwrap();

            // fd should still be valid because autoclose: false
            // fcntl(fd, F_GETFD) returns -1 with EBADF for closed fds
            let ret = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(
                ret, -1,
                "fd should still be open after Ruby GC (autoclose: false)"
            );
        });
    }

    #[test]
    #[serial]
    fn test_create_ruby_io_pipe_drop_closes_fd() {
        // Verifies PipeNotify::drop is the one that closes the fd
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let fd = pipe.read_fd();
            let _io = create_ruby_io(ruby, fd).unwrap();

            // Drop the pipe — should close the fd
            drop(pipe);

            let ret = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_eq!(ret, -1, "fd should be closed after PipeNotify::drop");
        });
    }

    // ========== Step 6: IO.select + pipe integration ==========

    #[test]
    #[serial]
    fn test_io_select_wakes_on_pipe_notify() {
        // IO.select should return when the pipe becomes readable after notify()
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Notify from another thread after a short delay
            let pipe_clone = pipe.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                pipe_clone.notify();
            });

            let start = Instant::now();
            // IO.select([read_io], nil, nil) — blocks until readable
            let result: Value = io_class.funcall("select", (select_arr, nil, nil)).unwrap();
            let elapsed = start.elapsed();

            assert!(
                !result.is_nil(),
                "IO.select should return non-nil on readable"
            );
            assert!(
                elapsed >= Duration::from_millis(30),
                "IO.select should have waited for notify, took {elapsed:?}"
            );
            assert!(
                elapsed < Duration::from_secs(2),
                "IO.select should not hang — took {elapsed:?}"
            );

            pipe.drain();
        });
    }

    #[test]
    #[serial]
    fn test_io_select_returns_nil_on_timeout_without_notify() {
        // IO.select with timeout returns nil if pipe is not notified
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            let start = Instant::now();
            // IO.select([read_io], nil, nil, 0.05) — 50ms timeout
            let result: Value = io_class
                .funcall("select", (select_arr, nil, nil, 0.05))
                .unwrap();
            let elapsed = start.elapsed();

            assert!(result.is_nil(), "IO.select should return nil on timeout");
            assert!(
                elapsed >= Duration::from_millis(30),
                "IO.select should have waited for timeout, took {elapsed:?}"
            );
        });
    }

    #[test]
    #[serial]
    fn test_io_select_immediate_if_already_notified() {
        // If pipe was notified before IO.select, it should return immediately
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            pipe.notify(); // Notify BEFORE select
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            let start = Instant::now();
            let result: Value = io_class
                .funcall("select", (select_arr, nil, nil, 1.0))
                .unwrap();
            let elapsed = start.elapsed();

            assert!(
                !result.is_nil(),
                "IO.select should return non-nil when already readable"
            );
            assert!(
                elapsed < Duration::from_millis(50),
                "IO.select should return immediately when pipe is already readable, took {elapsed:?}"
            );

            pipe.drain();
        });
    }

    #[test]
    #[serial]
    fn test_io_select_multiple_wake_cycles() {
        // Multiple notify → select → drain cycles work correctly
        with_ruby_python(|ruby, _api| {
            let pipe = PipeNotify::new().unwrap();
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            for _ in 0..5 {
                pipe.notify();
                let result: Value = io_class
                    .funcall("select", (select_arr, nil, nil, 1.0))
                    .unwrap();
                assert!(!result.is_nil(), "IO.select should wake on each notify");
                pipe.drain();
            }

            // After all drains, pipe should be empty — select with timeout returns nil
            let result: Value = io_class
                .funcall("select", (select_arr, nil, nil, 0.01))
                .unwrap();
            assert!(
                result.is_nil(),
                "pipe should be empty after all drain cycles"
            );
        });
    }

    #[test]
    #[serial]
    fn test_io_select_with_channel_drain_pattern() {
        // Test the full pipe + channel coordination pattern used by each_fiber_aware:
        // producer sends item + notifies pipe, consumer does IO.select + drain + try_recv
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let (tx, rx) = bounded::<StreamItem>(16);
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Producer: send items + notify for each
            let pipe_clone = pipe.clone();
            std::thread::spawn(move || {
                for i in 0..5 {
                    tx.send(StreamItem::Value(SendableValue::Integer(i)))
                        .unwrap();
                    pipe_clone.notify();
                    std::thread::sleep(Duration::from_millis(10));
                }
                tx.send(StreamItem::End).unwrap();
                pipe_clone.notify();
            });

            // Consumer: IO.select → drain → try_recv loop
            let mut collected = Vec::new();
            'outer: loop {
                let _: Value = io_class
                    .funcall("select", (select_arr, nil, nil, 2.0))
                    .unwrap();
                pipe.drain();

                loop {
                    match rx.try_recv() {
                        Ok(StreamItem::Value(v)) => {
                            if let SendableValue::Integer(n) = v {
                                collected.push(n);
                            }
                        }
                        Ok(StreamItem::End) => break 'outer,
                        Ok(StreamItem::Error(e)) => panic!("unexpected error: {e}"),
                        Err(crossbeam_channel::TryRecvError::Empty) => break,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => break 'outer,
                    }
                }
            }

            assert_eq!(
                collected,
                vec![0, 1, 2, 3, 4],
                "all items should arrive in order"
            );
        });
    }

    #[test]
    #[serial]
    fn test_io_select_batch_notify_drains_all() {
        // Producer sends multiple items between consumer wakeups.
        // Consumer must drain ALL bytes from pipe and ALL items from channel.
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let (tx, rx) = bounded::<StreamItem>(16);
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Producer: send 10 items rapidly with notifications, then End
            let pipe_clone = pipe.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    tx.send(StreamItem::Value(SendableValue::Integer(i)))
                        .unwrap();
                    pipe_clone.notify();
                }
                tx.send(StreamItem::End).unwrap();
                pipe_clone.notify();
            });

            // Give producer time to send everything
            std::thread::sleep(Duration::from_millis(50));

            // One IO.select + drain should get everything
            let _: Value = io_class
                .funcall("select", (select_arr, nil, nil, 2.0))
                .unwrap();
            pipe.drain();

            let mut collected = Vec::new();
            loop {
                match rx.try_recv() {
                    Ok(StreamItem::Value(v)) => {
                        if let SendableValue::Integer(n) = v {
                            collected.push(n);
                        }
                    }
                    Ok(StreamItem::End) => break,
                    Ok(StreamItem::Error(e)) => panic!("unexpected error: {e}"),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }

            assert_eq!(
                collected,
                (0..10).collect::<Vec<_>>(),
                "draining should collect all items sent in a batch"
            );
        });
    }

    #[test]
    #[serial]
    fn test_io_select_no_byte_accumulation() {
        // After proper drain cycles, no stale notification bytes remain
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let (tx, rx) = bounded::<StreamItem>(64);
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Send 50 items, each with a notification
            for i in 0..50 {
                tx.send(StreamItem::Value(SendableValue::Integer(i)))
                    .unwrap();
                pipe.notify();
            }

            // Consume all items with proper drain pattern
            let mut count = 0;
            loop {
                let result: Value = io_class
                    .funcall("select", (select_arr, nil, nil, 0.1))
                    .unwrap();
                if result.is_nil() {
                    break; // Timeout — pipe is empty
                }
                pipe.drain();
                while let Ok(StreamItem::Value(_)) = rx.try_recv() {
                    count += 1;
                }
            }

            assert_eq!(count, 50, "all 50 items should be consumed");

            // Final check: IO.select with short timeout should return nil (no stale bytes)
            let result: Value = io_class
                .funcall("select", (select_arr, nil, nil, 0.01))
                .unwrap();
            assert!(
                result.is_nil(),
                "no stale notification bytes should remain after proper draining"
            );
        });
    }

    // ========== Step 6: has_fiber_scheduler ==========

    #[test]
    #[serial]
    fn test_has_fiber_scheduler_returns_false_by_default() {
        with_ruby_python(|ruby, _api| {
            assert!(
                !has_fiber_scheduler(ruby),
                "has_fiber_scheduler should return false when no scheduler is installed"
            );
        });
    }

    #[test]
    #[serial]
    fn test_has_fiber_scheduler_detects_nil_scheduler() {
        // Fiber.scheduler returns nil by default — verify our function handles it
        with_ruby_python(|ruby, _api| {
            let fiber_class: Value = ruby.class_object().const_get("Fiber").unwrap();
            let scheduler: Value = fiber_class.funcall("scheduler", ()).unwrap();
            assert!(
                scheduler.is_nil(),
                "Fiber.scheduler should be nil by default"
            );
            assert!(!has_fiber_scheduler(ruby));
        });
    }

    // ========== Step 6: each_fiber_aware building blocks ==========

    #[test]
    #[serial]
    fn test_io_select_with_error_item() {
        // Verify the drain pattern correctly surfaces errors from the channel
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let (tx, rx) = bounded::<StreamItem>(16);
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Send a value, then an error, with notifications
            tx.send(StreamItem::Value(SendableValue::Integer(1)))
                .unwrap();
            pipe.notify();
            tx.send(StreamItem::Error("stream failed".to_string()))
                .unwrap();
            pipe.notify();

            let _: Value = io_class
                .funcall("select", (select_arr, nil, nil, 1.0))
                .unwrap();
            pipe.drain();

            // First item should be the value
            match rx.try_recv().unwrap() {
                StreamItem::Value(SendableValue::Integer(n)) => assert_eq!(n, 1),
                _ => panic!("expected Integer(1)"),
            }

            // Second item should be the error
            match rx.try_recv().unwrap() {
                StreamItem::Error(msg) => assert_eq!(msg, "stream failed"),
                _ => panic!("expected Error"),
            }
        });
    }

    #[test]
    #[serial]
    fn test_io_select_with_disconnect() {
        // If the producer drops the sender, try_recv returns Disconnected
        with_ruby_python(|ruby, _api| {
            let pipe = Arc::new(PipeNotify::new().unwrap());
            let (tx, rx) = bounded::<StreamItem>(16);
            let io = create_ruby_io(ruby, pipe.read_fd()).unwrap();
            let select_arr = ruby.ary_new_from_values(&[io]);
            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            // Send one value, notify, then drop sender
            tx.send(StreamItem::Value(SendableValue::Integer(99)))
                .unwrap();
            pipe.notify();
            drop(tx);

            let _: Value = io_class
                .funcall("select", (select_arr, nil, nil, 1.0))
                .unwrap();
            pipe.drain();

            // Get the value
            match rx.try_recv().unwrap() {
                StreamItem::Value(SendableValue::Integer(n)) => assert_eq!(n, 99),
                _ => panic!("expected Integer(99)"),
            }

            // Next try_recv should indicate disconnected
            assert!(matches!(
                rx.try_recv(),
                Err(crossbeam_channel::TryRecvError::Disconnected)
            ));
        });
    }

    #[test]
    #[serial]
    fn test_create_ruby_io_multiple_pipes() {
        // Multiple Ruby IO objects wrapping different pipes should work independently
        with_ruby_python(|ruby, _api| {
            let pipe1 = PipeNotify::new().unwrap();
            let pipe2 = PipeNotify::new().unwrap();

            let io1 = create_ruby_io(ruby, pipe1.read_fd()).unwrap();
            let io2 = create_ruby_io(ruby, pipe2.read_fd()).unwrap();

            let fileno1: i32 = io1.funcall("fileno", ()).unwrap();
            let fileno2: i32 = io2.funcall("fileno", ()).unwrap();

            assert_ne!(
                fileno1, fileno2,
                "different pipes should have different fds"
            );

            // Notify pipe1 only — IO.select on io1 should return, io2 should not
            pipe1.notify();

            let nil = ruby.qnil().as_value();
            let io_class: Value = ruby.class_object().const_get("IO").unwrap();

            let arr1 = ruby.ary_new_from_values(&[io1]);
            let result1: Value = io_class.funcall("select", (arr1, nil, nil, 0.1)).unwrap();
            assert!(
                !result1.is_nil(),
                "io1 should be readable after pipe1.notify()"
            );

            let arr2 = ruby.ary_new_from_values(&[io2]);
            let result2: Value = io_class.funcall("select", (arr2, nil, nil, 0.01)).unwrap();
            assert!(
                result2.is_nil(),
                "io2 should NOT be readable (pipe2 not notified)"
            );

            pipe1.drain();
        });
    }

    // ========== Step 7: each() auto-dispatch (needs Ruby class registration) ==========

    /// Register NonBlockingStream as a Ruby class so we can test `each` with a block.
    /// Idempotent — safe to call multiple times.
    fn ensure_nb_class_registered(ruby: &Ruby) {
        let rubyx = ruby.define_module("Rubyx").unwrap();
        let class = rubyx
            .define_class("NonBlockingStream", ruby.class_object())
            .unwrap();
        // define_method is idempotent (replaces if exists)
        class
            .define_method("each", magnus::method!(NonBlockingStream::each, 0))
            .unwrap();
        class.include_module(ruby.module_enumerable()).unwrap();
    }

    /// Wrap a NonBlockingStream as a Ruby object so we can call Ruby methods on it.
    fn wrap_stream(ruby: &Ruby, stream: NonBlockingStream) -> Value {
        use magnus::IntoValue;
        stream.into_value_with(ruby)
    }

    #[test]
    #[serial]
    fn test_each_dispatches_gvl_release_without_scheduler() {
        // Without a Fiber Scheduler, each() should use the GVL-release path
        // and produce correct results via to_a (which calls each with a block)
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            for i in 0..5 {
                tx.send(StreamItem::Value(SendableValue::Integer(i)))
                    .unwrap();
            }
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            // to_a calls each internally, which will use GVL-release path
            // (no Fiber Scheduler installed)
            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();

            assert_eq!(arr.len(), 5);
            for i in 0..5 {
                let v = i64::try_convert(arr.entry::<Value>(i).unwrap()).unwrap();
                assert_eq!(v, i as i64);
            }
        });
    }

    #[test]
    #[serial]
    fn test_each_empty_stream() {
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 0, "empty stream should produce empty array");
        });
    }

    #[test]
    #[serial]
    fn test_each_single_value() {
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            tx.send(StreamItem::Value(SendableValue::Str("hello".to_string())))
                .unwrap();
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(
                String::try_convert(arr.entry::<Value>(0).unwrap()).unwrap(),
                "hello"
            );
        });
    }

    #[test]
    #[serial]
    fn test_each_mixed_types() {
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            tx.send(StreamItem::Value(SendableValue::Integer(42)))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Float(
                std::f64::consts::PI,
            )))
            .unwrap();
            tx.send(StreamItem::Value(SendableValue::Str("test".to_string())))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Bool(true)))
                .unwrap();
            tx.send(StreamItem::Value(SendableValue::Nil)).unwrap();
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 5);

            assert_eq!(
                i64::try_convert(arr.entry::<Value>(0).unwrap()).unwrap(),
                42
            );
            assert!(
                (f64::try_convert(arr.entry::<Value>(1).unwrap()).unwrap() - std::f64::consts::PI)
                    .abs()
                    < 1e-9
            );
            assert_eq!(
                String::try_convert(arr.entry::<Value>(2).unwrap()).unwrap(),
                "test"
            );
            assert!(bool::try_convert(arr.entry::<Value>(3).unwrap()).unwrap());
            assert!(arr.entry::<Value>(4).unwrap().is_nil());
        });
    }

    #[test]
    #[serial]
    fn test_each_propagates_error() {
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            tx.send(StreamItem::Value(SendableValue::Integer(1)))
                .unwrap();
            tx.send(StreamItem::Error("stream exploded".to_string()))
                .unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let err = ruby_obj.funcall::<_, _, Value>("to_a", ()).unwrap_err();
            assert!(
                err.to_string().contains("stream exploded"),
                "error should propagate: {err}"
            );
        });
    }

    #[test]
    #[serial]
    fn test_each_with_delayed_producer() {
        // Producer sends values after a delay — each() should wait via GVL release
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());

            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                for i in 0..3 {
                    tx.send(StreamItem::Value(SendableValue::Integer(i)))
                        .unwrap();
                }
                tx.send(StreamItem::End).unwrap();
            });

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let start = Instant::now();
            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let elapsed = start.elapsed();

            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 3);
            assert!(
                elapsed >= Duration::from_millis(30),
                "should have waited for producer, took {elapsed:?}"
            );
        });
    }

    #[test]
    #[serial]
    fn test_each_disconnect_returns_empty() {
        // If producer drops sender immediately, each() should return without error
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = bounded::<StreamItem>(16);
            let pipe = Arc::new(PipeNotify::new().unwrap());
            drop(tx);

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(
                arr.len(),
                0,
                "disconnected channel should produce empty array"
            );
        });
    }

    #[test]
    #[serial]
    fn test_each_enumerable_first() {
        // Enumerable#first should work (calls each, takes first N, breaks)
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            for i in 0..10 {
                tx.send(StreamItem::Value(SendableValue::Integer(i)))
                    .unwrap();
            }
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("first", (3,)).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 3);
            assert_eq!(i64::try_convert(arr.entry::<Value>(0).unwrap()).unwrap(), 0);
            assert_eq!(i64::try_convert(arr.entry::<Value>(1).unwrap()).unwrap(), 1);
            assert_eq!(i64::try_convert(arr.entry::<Value>(2).unwrap()).unwrap(), 2);
        });
    }

    #[test]
    #[serial]
    fn test_each_uses_gvl_release_path_without_scheduler() {
        // Confirm has_fiber_scheduler returns false AND each() still works.
        // This proves the GVL-release dispatch path is exercised.
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            assert!(
                !has_fiber_scheduler(ruby),
                "precondition: no Fiber Scheduler should be installed"
            );

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());
            tx.send(StreamItem::Value(SendableValue::Integer(99)))
                .unwrap();
            tx.send(StreamItem::End).unwrap();

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(
                i64::try_convert(arr.entry::<Value>(0).unwrap()).unwrap(),
                99
            );
        });
    }

    #[test]
    #[serial]
    fn test_each_large_stream() {
        with_ruby_python(|ruby, _api| {
            ensure_nb_class_registered(ruby);

            let (tx, rx) = unbounded();
            let pipe = Arc::new(PipeNotify::new().unwrap());

            std::thread::spawn(move || {
                for i in 0..1000 {
                    tx.send(StreamItem::Value(SendableValue::Integer(i)))
                        .unwrap();
                }
                tx.send(StreamItem::End).unwrap();
            });

            let stream = NonBlockingStream::new(rx, pipe);
            let ruby_obj = wrap_stream(ruby, stream);

            let result: Value = ruby_obj.funcall("to_a", ()).unwrap();
            let arr = RArray::try_convert(result).unwrap();
            assert_eq!(arr.len(), 1000);

            // Verify order: check first, middle, last
            assert_eq!(i64::try_convert(arr.entry::<Value>(0).unwrap()).unwrap(), 0);
            assert_eq!(
                i64::try_convert(arr.entry::<Value>(500).unwrap()).unwrap(),
                500
            );
            assert_eq!(
                i64::try_convert(arr.entry::<Value>(999).unwrap()).unwrap(),
                999
            );
        });
    }
}
