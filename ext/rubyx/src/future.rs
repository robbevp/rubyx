use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::ruby_helpers::runtime_error;
use crate::rubyx_object::python_to_sendable;
use crate::stream::SendableValue;
use crossbeam_channel::{bounded, Receiver};
use magnus::{Error, Value};
use std::cell::RefCell;
use std::thread::{self, JoinHandle};

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
        // Join the worker thread first
        if let Some(handle) = self.handle.borrow_mut().take() {
            let _ = handle.join();
        }

        match self.receiver.try_recv() {
            Ok(Ok(sendable)) => sendable.try_into(),
            Ok(Err(err)) => Err(Error::new(runtime_error(), err)),
            Err(_) => Err(Error::new(
                runtime_error(),
                "Future already consumed or worker failed",
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
