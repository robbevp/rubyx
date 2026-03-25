use crate::gvl::{rb_thread_call_without_gvl, rb_thread_check_ints, recv_loop, ubf_cancel};
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::ruby_helpers::runtime_error;
use crate::rubyx_object::python_to_sendable;
use crate::stream::SendableValue;
use crossbeam_channel::{bounded, Receiver};
use magnus::{Error, Value};
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

struct FutureRecvArgs {
    receiver: Receiver<Result<SendableValue, String>>,
    result: Option<Result<Result<SendableValue, String>, crossbeam_channel::RecvError>>,
    cancel: Arc<AtomicBool>,
}

unsafe extern "C" fn future_recv_cb(args: *mut c_void) -> *mut c_void {
    let args = &mut *(args as *mut FutureRecvArgs);
    args.result = recv_loop(&args.receiver, &args.cancel);
    std::ptr::null_mut()
}

/// A future representing an async Python operation running on a background thread.
///
/// The Python coroutine is executed via `asyncio.run()` on a dedicated thread.
/// The Ruby thread is free to do other work. Call `value` to block until the
/// result is ready, or `ready?` to check without blocking.
#[magnus::wrap(class = "Rubyx::Future", free_immediately)]
pub(crate) struct RubyxFuture {
    receiver: Receiver<Result<SendableValue, String>>,
    handle: RefCell<Option<JoinHandle<()>>>,
}

unsafe impl Send for RubyxFuture {}
unsafe impl Sync for RubyxFuture {}

impl RubyxFuture {
    /// Spawn a background thread that runs asyncio.run(coroutine).
    pub fn from_coroutine(py_coroutine: *mut PyObject, api: &'static PythonApi) -> Self {
        let (tx, rx) = bounded(1);
        let coroutine_addr = py_coroutine as usize;

        api.incref(py_coroutine);

        let handle = thread::spawn(move || {
            let coroutine = coroutine_addr as *mut PyObject;
            let api = crate::api();
            let gil = api.ensure_gil();

            let result = run_asyncio_sendable(coroutine, api);

            api.decref(coroutine);
            api.release_gil(gil);

            let _ = tx.send(result);
        });

        Self {
            receiver: rx,
            handle: RefCell::new(Some(handle)),
        }
    }

    /// Block until the result is ready and return it as a Ruby value.
    /// Can only be called once — subsequent calls return an error.
    pub fn value(&self) -> Result<Value, Error> {
        if let Ok(result) = self.receiver.try_recv() {
            return match result {
                Ok(sendable) => sendable.try_into(),
                Err(err) => Err(Error::new(runtime_error(), err)),
            };
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = FutureRecvArgs {
            receiver: self.receiver.clone(),
            result: None,
            cancel: cancel.clone(),
        };

        unsafe {
            rb_thread_call_without_gvl(
                future_recv_cb,
                &mut args as *mut FutureRecvArgs as *mut c_void,
                Some(ubf_cancel),
                Arc::as_ptr(&cancel) as *mut c_void,
            );
            rb_thread_check_ints();
        }

        if let Some(handle) = self.handle.borrow_mut().take() {
            let _ = handle.join();
        }

        match args.result {
            Some(Ok(Ok(sendable))) => sendable.try_into(),
            Some(Ok(Err(err))) => Err(Error::new(runtime_error(), err)),
            _ => Err(Error::new(
                runtime_error(),
                "Future cancelled or worker failed",
            )),
        }
    }

    pub fn is_ready(&self) -> bool {
        !self.receiver.is_empty()
    }
}

impl Drop for RubyxFuture {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.borrow_mut().take() {
            let _ = handle.join();
        }
    }
}

/// Run asyncio.run(coroutine) and convert the result to a SendableValue
/// (thread-safe). Runs on the worker thread with the GIL held.
fn run_asyncio_sendable(
    coroutine: *mut PyObject,
    api: &PythonApi,
) -> Result<SendableValue, String> {
    let asyncio = api
        .import_module("asyncio")
        .map_err(|e| format!("Failed to import asyncio: {e}"))?;
    let run_fn = api.object_get_attr_string(asyncio, "run");

    if run_fn.is_null() {
        api.clear_error();
        api.decref(asyncio);
        return Err("asyncio.run not found".to_string());
    }

    let args = unsafe { (api.py_tuple_new)(1) };
    if args.is_null() {
        api.decref(run_fn);
        api.decref(asyncio);
        return Err("Failed to allocate argument tuple".to_string());
    }
    api.incref(coroutine);
    unsafe { (api.py_tuple_set_item)(args, 0, coroutine) };

    let result = api.object_call(run_fn, args, std::ptr::null_mut());
    api.decref(args);
    api.decref(run_fn);
    api.decref(asyncio);

    if result.is_null() {
        let err = if let Some(exc) = PythonApi::extract_exception(api) {
            exc.to_string()
        } else {
            "Python async call failed".to_string()
        };
        return Err(err);
    }

    let sendable = python_to_sendable(result, api);
    api.decref(result);
    sendable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::SendableValue;
    use crate::test_helpers::with_ruby_python;
    use crossbeam_channel::{bounded, unbounded};
    use magnus::value::ReprValue;
    use magnus::TryConvert;
    use serial_test::serial;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    // ========== future_recv_cb (pure Rust, no Ruby needed) ==========

    #[test]
    fn test_future_recv_cb_delivers_ok_value() {
        let (tx, rx) = unbounded();
        tx.send(Ok(SendableValue::Integer(42))).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = FutureRecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }

        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Integer(n)) => assert_eq!(n, 42),
            other => panic!("expected Ok(Integer(42)), got {other:?}"),
        }
    }

    #[test]
    fn test_future_recv_cb_delivers_err_value() {
        let (tx, rx) = unbounded();
        tx.send(Err("python exploded".to_string())).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = FutureRecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }

        match args.result.unwrap().unwrap() {
            Err(msg) => assert_eq!(msg, "python exploded"),
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn test_future_recv_cb_handles_disconnect() {
        let (tx, rx) = bounded::<Result<SendableValue, String>>(1);
        drop(tx);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = FutureRecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }

        assert!(args.result.unwrap().is_err());
    }

    #[test]
    fn test_future_recv_cb_respects_cancel_flag() {
        let (_tx, rx) = bounded::<Result<SendableValue, String>>(1);

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();

        let mut args = FutureRecvArgs {
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
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        let elapsed = start.elapsed();

        assert!(args.result.is_none(), "expected None on cancel");
        assert!(
            elapsed < Duration::from_millis(200),
            "cancel took {elapsed:?}, expected < 200ms"
        );
    }

    #[test]
    fn test_future_recv_cb_cancel_flag_already_set() {
        let (_tx, rx) = bounded::<Result<SendableValue, String>>(1);

        let cancel = Arc::new(AtomicBool::new(true)); // pre-set
        let mut args = FutureRecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        let start = Instant::now();
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        let elapsed = start.elapsed();

        assert!(args.result.is_none(), "expected None on pre-set cancel");
        assert!(
            elapsed < Duration::from_millis(10),
            "pre-set cancel took {elapsed:?}, expected near-instant"
        );
    }

    #[test]
    fn test_future_recv_cb_with_delayed_producer() {
        let (tx, rx) = bounded::<Result<SendableValue, String>>(1);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut args = FutureRecvArgs {
            receiver: rx,
            result: None,
            cancel,
        };

        // Send value after a delay (simulates slow Python computation)
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            tx.send(Ok(SendableValue::Str("delayed".to_string())))
                .unwrap();
        });

        let start = Instant::now();
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        let elapsed = start.elapsed();

        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Str(s)) => assert_eq!(s, "delayed"),
            other => panic!("expected Ok(Str), got {other:?}"),
        }
        assert!(
            elapsed >= Duration::from_millis(80),
            "should have waited for producer, elapsed: {elapsed:?}"
        );
    }

    #[test]
    fn test_future_recv_cb_all_sendable_types() {
        let (tx, rx) = unbounded();
        tx.send(Ok(SendableValue::Nil)).unwrap();
        tx.send(Ok(SendableValue::Integer(99))).unwrap();
        tx.send(Ok(SendableValue::Float(3.14))).unwrap();
        tx.send(Ok(SendableValue::Str("hello".to_string())))
            .unwrap();
        tx.send(Ok(SendableValue::Bool(true))).unwrap();

        let cancel = Arc::new(AtomicBool::new(false));

        // Nil
        let mut args = FutureRecvArgs {
            receiver: rx.clone(),
            result: None,
            cancel: cancel.clone(),
        };
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        assert!(matches!(
            args.result.unwrap().unwrap(),
            Ok(SendableValue::Nil)
        ));

        // Integer
        let mut args = FutureRecvArgs {
            receiver: rx.clone(),
            result: None,
            cancel: cancel.clone(),
        };
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Integer(n)) => assert_eq!(n, 99),
            other => panic!("expected Integer(99), got {other:?}"),
        }

        // Float
        let mut args = FutureRecvArgs {
            receiver: rx.clone(),
            result: None,
            cancel: cancel.clone(),
        };
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Float(f)) => assert!((f - 3.14).abs() < f64::EPSILON),
            other => panic!("expected Float(3.14), got {other:?}"),
        }

        // Str
        let mut args = FutureRecvArgs {
            receiver: rx.clone(),
            result: None,
            cancel: cancel.clone(),
        };
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Str(s)) => assert_eq!(s, "hello"),
            other => panic!("expected Str(\"hello\"), got {other:?}"),
        }

        // Bool
        let mut args = FutureRecvArgs {
            receiver: rx.clone(),
            result: None,
            cancel: cancel.clone(),
        };
        unsafe {
            future_recv_cb(&mut args as *mut FutureRecvArgs as *mut c_void);
        }
        match args.result.unwrap().unwrap() {
            Ok(SendableValue::Bool(b)) => assert!(b),
            other => panic!("expected Bool(true), got {other:?}"),
        }
    }

    // ========== RubyxFuture::value (needs Ruby for SendableValue → Value) ==========

    /// Helper: create a RubyxFuture backed by a channel (no Python needed)
    fn make_future(rx: Receiver<Result<SendableValue, String>>) -> RubyxFuture {
        RubyxFuture {
            receiver: rx,
            handle: RefCell::new(None),
        }
    }

    /// Helper: create a RubyxFuture with a JoinHandle
    fn make_future_with_handle(
        rx: Receiver<Result<SendableValue, String>>,
        handle: JoinHandle<()>,
    ) -> RubyxFuture {
        RubyxFuture {
            receiver: rx,
            handle: RefCell::new(Some(handle)),
        }
    }

    #[test]
    #[serial]
    fn test_value_fast_path_already_ready() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Integer(7))).unwrap();

            let future = make_future(rx);
            let val = future.value().unwrap();
            assert_eq!(i64::try_convert(val).unwrap(), 7);
        });
    }

    #[test]
    #[serial]
    fn test_value_fast_path_error() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = bounded(1);
            tx.send(Err("boom".to_string())).unwrap();

            let future = make_future(rx);
            let err = future.value().unwrap_err();
            assert!(err.to_string().contains("boom"));
        });
    }

    #[test]
    #[serial]
    fn test_value_slow_path_waits_for_producer() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = bounded(1);

            let handle = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                tx.send(Ok(SendableValue::Str("from worker".to_string())))
                    .unwrap();
            });

            let future = make_future_with_handle(rx, handle);

            let start = Instant::now();
            let val = future.value().unwrap();
            let elapsed = start.elapsed();

            let s: String = TryConvert::try_convert(val).unwrap();
            assert_eq!(s, "from worker");
            assert!(
                elapsed >= Duration::from_millis(80),
                "should have waited, elapsed: {elapsed:?}"
            );
        });
    }

    #[test]
    #[serial]
    fn test_value_returns_all_sendable_types() {
        with_ruby_python(|_ruby, _api| {
            // Integer
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Integer(42))).unwrap();
            let val = make_future(rx).value().unwrap();
            assert_eq!(i64::try_convert(val).unwrap(), 42);

            // Float
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Float(2.5))).unwrap();
            let val = make_future(rx).value().unwrap();
            assert_eq!(f64::try_convert(val).unwrap(), 2.5);

            // String
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Str("hello".to_string())))
                .unwrap();
            let val = make_future(rx).value().unwrap();
            let s: String = TryConvert::try_convert(val).unwrap();
            assert_eq!(s, "hello");

            // Bool
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Bool(true))).unwrap();
            let val = make_future(rx).value().unwrap();
            assert_eq!(bool::try_convert(val).unwrap(), true);

            // Nil
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Nil)).unwrap();
            let val = make_future(rx).value().unwrap();
            assert!(val.is_nil());
        });
    }

    #[test]
    #[serial]
    fn test_value_consumed_twice_returns_error() {
        with_ruby_python(|_ruby, _api| {
            let (tx, rx) = bounded(1);
            tx.send(Ok(SendableValue::Integer(1))).unwrap();

            let future = make_future(rx);
            let _ = future.value().unwrap();
            let err = future.value().unwrap_err();
            assert!(err.to_string().contains("consumed") || err.to_string().contains("failed"));
        });
    }

    #[test]
    fn test_is_ready_before_and_after_send() {
        let (tx, rx) = bounded(1);
        let future = make_future(rx);

        assert!(!future.is_ready());
        tx.send(Ok(SendableValue::Integer(1))).unwrap();
        assert!(future.is_ready());
    }

    #[test]
    fn test_drop_joins_handle() {
        let (tx, rx) = bounded(1);
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();

        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag_clone.store(true, Ordering::Relaxed);
            let _ = tx.send(Ok(SendableValue::Nil));
        });

        let future = make_future_with_handle(rx, handle);
        drop(future); // should join the handle

        assert!(
            flag.load(Ordering::Relaxed),
            "worker should have completed before drop returned"
        );
    }
}
