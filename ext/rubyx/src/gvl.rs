use crossbeam_channel::Receiver;
use std::ffi::c_void;
use std::sync::atomic::AtomicBool;

extern "C" {
    /// Release the GVL, call `func(data1)`, then reacquire the GVL.
    ///
    /// While `func` runs, other Ruby threads can execute. Inside `func`,
    /// you must NOT call any Ruby C API or access any Ruby VALUE.
    ///
    /// If `ubf` is provided, Ruby may call it from another thread to
    /// interrupt `func` (e.g., on Thread#kill or signal delivery).
    ///
    /// # Safety
    ///
    /// - `func` must not touch Ruby objects (GVL is not held).
    /// - `data1` must remain valid for the duration of `func`.
    /// - `data2` must remain valid for the duration of `ubf` (if provided).
    pub(crate) fn rb_thread_call_without_gvl(
        func: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        data1: *mut c_void,
        ubf: Option<unsafe extern "C" fn(*mut c_void)>,
        data2: *mut c_void,
    ) -> *mut c_void;

    /// Check for pending Ruby interrupts (Thread#kill, signals, etc.).
    ///
    /// Must be called WITH the GVL held. If an interrupt is pending,
    /// this raises a Ruby exception (longjmp). Call this immediately
    /// after `rb_thread_call_without_gvl` returns to deliver any
    /// interrupts that arrived while the GVL was released.
    pub(crate) fn rb_thread_check_ints();

}

pub(crate) unsafe extern "C" fn ubf_cancel(data: *mut c_void) {
    let cancel = &*(data as *const AtomicBool);
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Generic receive loop for use inside `rb_thread_call_without_gvl` callbacks.
///
/// Polls the channel with a 50ms timeout between cancel-flag checks.
/// Returns `None` if cancelled, `Some(Ok(item))` on success, or
/// `Some(Err(_))` if the sender disconnected.
pub(crate) fn recv_loop<T>(
    receiver: &Receiver<T>,
    cancel: &AtomicBool,
) -> Option<Result<T, crossbeam_channel::RecvError>> {
    loop {
        match receiver.try_recv() {
            Ok(item) => return Some(Ok(item)),
            Err(crossbeam_channel::TryRecvError::Empty) => {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return None;
                }
                match receiver.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(item) => return Some(Ok(item)),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        return Some(Err(crossbeam_channel::RecvError));
                    }
                }
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Some(Err(crossbeam_channel::RecvError));
            }
        }
    }
}
