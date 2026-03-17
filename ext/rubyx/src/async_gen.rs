use crate::api;
use crate::exception::PythonException;
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::python_guard::PyGuard;
use crate::rubyx_object::python_to_sendable;
use crate::stream::StreamItem;
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread;
use std::thread::JoinHandle;

pub(crate) const SYNC_ADAPTER_PY: &str = include_str!("python/sync_adapter.py");

/// A stream that can consume either sync or async Python generators
///
/// Sync generators, PyIter_Next loop.
/// Async generators, Rust-side driving depending on configuration
pub struct AsyncGeneratorStream {
    receiver: Receiver<StreamItem>,
    cancel_sender: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

/// Strategy for consuming async generators
#[derive(Clone, Copy, Debug)]
pub enum AsyncStrategy {
    /// AsyncToSync adapter, then use PyIter_Next loop
    PythonAdapter,
    /// Drive __anext__() coroutines from Rust with asyncio
    RustDriving,
}

impl Drop for AsyncGeneratorStream {
    fn drop(&mut self) {
        // Signal the worker thread to stop
        let _ = self.cancel_sender.try_send(());
        // Drain the channel so the worker doesn't block on a full send
        while self.receiver.try_recv().is_ok() {}
        // Join the worker thread to ensure GIL is released
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
impl Iterator for AsyncGeneratorStream {
    type Item = Result<magnus::Value, magnus::Error>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.receiver.recv() {
            Ok(StreamItem::Value(v)) => Some(v.try_into()),
            Ok(StreamItem::Error(e)) => Some(Err(magnus::Error::new(
                crate::ruby_helpers::runtime_error(),
                e,
            ))),
            Ok(StreamItem::End) | Err(_) => None,
        }
    }
}

impl AsyncGeneratorStream {
    /// Create a stream from Python object
    pub fn from_python_object(
        py_obj: *mut PyObject,
        async_strategy: AsyncStrategy,
    ) -> Result<Self, String> {
        let api = api();
        let gil = api.ensure_gil();
        let result = if api.is_async_iterable(py_obj) {
            match async_strategy {
                AsyncStrategy::PythonAdapter => Self::from_async_via_adapter(py_obj, api),
                AsyncStrategy::RustDriving => Self::from_async_via_rust(py_obj, api),
            }
        } else {
            let py_iter = api.object_get_iter(py_obj);
            if py_iter.is_null() {
                Err("Object is not iterable".to_string())
            } else {
                Ok(Self::from_sync_iterator(py_iter))
            }
        };
        api.release_gil(gil);
        result
    }
    /// Async path using AsyncToSync adapter.
    fn from_async_via_adapter(async_gen: *mut PyObject, api: &PythonApi) -> Result<Self, String> {
        let sync_iter = api.wrap_async_generator(async_gen);
        if sync_iter.is_null() {
            return Err("Failed to wrap async generator".to_string());
        }
        Ok(Self::from_sync_iterator(sync_iter))
    }
    /// Async path using Rust-side event loop driving.
    fn from_async_via_rust(async_gen: *mut PyObject, api: &PythonApi) -> Result<Self, String> {
        let (value_tx, value_rx) = unbounded();
        let (cancel_tx, cancel_rx) = bounded(1);
        // worker thread own the reference - incref
        api.incref(async_gen);
        let gen_ptr = async_gen as usize;
        let handle = thread::spawn(move || {
            let api = crate::api();
            let gil = api.ensure_gil();
            let async_gen = gen_ptr as *mut PyObject;
            drive_async_generator(api, async_gen, &value_tx, &cancel_rx);
            api.release_gil(gil);
        });
        Ok(Self {
            receiver: value_rx,
            cancel_sender: cancel_tx,
            handle: Some(handle),
        })
    }
    fn from_sync_iterator(py_iter: *mut PyObject) -> Self {
        let (value_tx, value_rx) = unbounded();
        let (cancel_tx, cancel_rx) = bounded(1);

        // Cast the raw pointer to usize so it can cross the thread boundary.
        // *mut PyObject is not Send, but the usize value is just a number.
        // This is safe because the worker thread will acquire the GIL before
        // using the pointer, and the pointer remains valid (Python iterator
        // is kept alive by its refcount).
        let py_iter_addr = py_iter as usize;

        let handle = thread::spawn(move || {
            let py_iter = py_iter_addr as *mut PyObject;
            // Worker thread: acquire GIL, iterate, send values
            let api = api();
            let gil = api.ensure_gil();

            loop {
                // Check if there is a cancellation
                if cancel_rx.try_recv().is_ok() {
                    break;
                }

                // Get next item from Python iterator
                let item = api.iter_next(py_iter);
                if item.is_null() {
                    // Check if an exception was raised (vs normal exhaustion)
                    if api.has_error() {
                        if let Some(exc) = crate::python_api::PythonApi::extract_exception(api) {
                            value_tx.send(StreamItem::Error(exc.to_string())).ok();
                        } else {
                            value_tx.send(StreamItem::End).ok();
                        }
                    } else {
                        value_tx.send(StreamItem::End).ok();
                    }
                    break;
                }
                // Convert and send to ruby
                let ruby_value = python_to_sendable(item, &api)
                    .map_err(|e| format!("Error converting Python value to Ruby: {e}"));
                api.decref(item);
                match ruby_value {
                    Ok(value) => {
                        if value_tx.send(StreamItem::Value(value)).is_err() {
                            break; // Consumer dropped — stop producing
                        }
                    }
                    Err(e) => {
                        value_tx.send(StreamItem::Error(e)).ok();
                        break;
                    }
                }
            }
            api.decref(py_iter);
            api.release_gil(gil);
        });
        Self {
            receiver: value_rx,
            cancel_sender: cancel_tx,
            handle: Some(handle),
        }
    }
}

pub(crate) fn drive_async_generator(
    api: &PythonApi,
    async_gen: *mut PyObject,
    sender: &Sender<StreamItem>,
    cancel: &Receiver<()>,
) {
    if async_gen.is_null() {
        let _ = sender.send(StreamItem::Error("Async generator is null".to_string()));
        return;
    }

    let Some(_async_gen_guard) = PyGuard::new(async_gen, api) else {
        let _ = sender.send(StreamItem::Error("Async generator is null".to_string()));
        return;
    };

    let asyncio = match api.import_module("asyncio") {
        Ok(obj) => {
            let Some(guard) = PyGuard::new(obj, api) else {
                let _ = sender.send(StreamItem::Error("Failed to import asyncio".to_string()));
                return;
            };
            guard
        }
        Err(err) => {
            let _ = sender.send(StreamItem::Error(err.to_string()));
            return;
        }
    };
    let Some(new_loop_fun) = PyGuard::new(
        api.object_get_attr_string(asyncio.ptr(), "new_event_loop"),
        api,
    ) else {
        let _ = sender.send(StreamItem::Error(
            "Failed to get asyncio.new_event_loop".to_string(),
        ));
        if api.has_error() {
            api.clear_error();
        }
        return;
    };
    let Some(event_loop) = PyGuard::new(api.object_call_no_args(new_loop_fun.ptr()), api) else {
        let _ = sender.send(StreamItem::Error("Failed to create event loop".to_string()));
        if api.has_error() {
            api.clear_error();
        }
        return;
    };
    let Some(run_fn) = PyGuard::new(
        api.object_get_attr_string(event_loop.ptr(), "run_until_complete"),
        api,
    ) else {
        let _ = sender.send(StreamItem::Error(
            "Failed to get event_loop.run_until_complete".to_string(),
        ));
        if api.has_error() {
            api.clear_error();
        }
        return;
    };
    let Some(anext_method) = PyGuard::new(api.object_get_attr_string(async_gen, "__anext__"), api)
    else {
        let _ = sender.send(StreamItem::Error(
            "Failed to get __anext__ method".to_string(),
        ));
        if api.has_error() {
            api.clear_error();
        }
        return;
    };
    loop {
        if cancel.try_recv().is_ok() {
            break;
        }

        let coroutine = api.object_call_no_args(anext_method.ptr());
        if coroutine.is_null() {
            if let Some(exc) = PythonApi::extract_exception(api) {
                if is_stop_async_iteration(&exc) {
                    let _ = sender.send(StreamItem::End);
                } else {
                    let _ = sender.send(StreamItem::Error(exc.to_string()));
                }
            } else {
                let _ = sender.send(StreamItem::Error("__anext__() failed".into()));
                if api.has_error() {
                    api.clear_error();
                }
            }
            break;
        }

        let args_tuple = api.tuple_new(1);
        if args_tuple.is_null() {
            api.decref(coroutine);
            let _ = sender.send(StreamItem::Error(
                "Failed to allocate argument tuple".to_string(),
            ));
            if api.has_error() {
                api.clear_error();
            }
            break;
        }

        api.incref(coroutine);
        if api.tuple_set_item(args_tuple, 0, coroutine) != 0 {
            api.decref(args_tuple);
            api.decref(coroutine);
            api.decref(coroutine);
            let _ = sender.send(StreamItem::Error("Failed to set tuple item".to_string()));
            if api.has_error() {
                api.clear_error();
            }
            break;
        }

        let result = api.object_call(run_fn.ptr(), args_tuple, std::ptr::null_mut());
        api.decref(args_tuple);
        api.decref(coroutine);

        if result.is_null() {
            if let Some(exc) = PythonApi::extract_exception(api) {
                if is_stop_async_iteration(&exc) {
                    let _ = sender.send(StreamItem::End);
                } else {
                    let _ = sender.send(StreamItem::Error(exc.to_string()));
                }
            } else if api.has_error() {
                api.clear_error();
            } else {
                let _ = sender.send(StreamItem::Error(
                    "run_until_complete failed without Python exception".to_string(),
                ));
            }
            break;
        }

        match catch_unwind(AssertUnwindSafe(|| python_to_sendable(result, api))) {
            Ok(Ok(val)) => {
                api.decref(result);
                if sender.send(StreamItem::Value(val)).is_err() {
                    break;
                }
            }
            Ok(Err(err_msg)) => {
                api.decref(result);
                let _ = sender.send(StreamItem::Error(format!(
                    "Cannot convert Python value to Ruby: {err_msg}"
                )));
                break;
            }
            Err(_) => {
                api.decref(result);
                let _ = sender.send(StreamItem::Error(
                    "Cannot convert Python value to Ruby".to_string(),
                ));
                break;
            }
        }
    }

    if let Some(close_fn) = PyGuard::new(api.object_get_attr_string(event_loop.ptr(), "close"), api)
    {
        let close_result = api.object_call_no_args(close_fn.ptr());
        if !close_result.is_null() {
            drop(PyGuard::new(close_result, api));
        } else if api.has_error() {
            api.clear_error();
        }
    } else if api.has_error() {
        api.clear_error();
    }
}

fn is_stop_async_iteration(exc: &PythonException) -> bool {
    matches!(
        exc,
        PythonException::Exception {
            kind,
            message: _,
            traceback: _,
        } if kind == "StopAsyncIteration"
    )
}

#[cfg(test)]
impl AsyncGeneratorStream {
    /// Test constructor: create an AsyncGeneratorStream from a channel of `Option<SendableValue>`.
    /// `Some(val)` sends a value, `None` signals end-of-stream.
    pub(crate) fn from_channel(
        rx: Receiver<Option<crate::stream::SendableValue>>,
        cancel_tx: Sender<()>,
    ) -> Self {
        let (value_tx, value_rx) = unbounded();
        let handle = thread::spawn(move || {
            while let Ok(item) = rx.recv() {
                match item {
                    Some(val) => {
                        if value_tx.send(StreamItem::Value(val)).is_err() {
                            return;
                        }
                    }
                    None => {
                        value_tx.send(StreamItem::End).ok();
                        return;
                    }
                }
            }
            value_tx.send(StreamItem::End).ok();
        });
        Self {
            receiver: value_rx,
            cancel_sender: cancel_tx,
            handle: Some(handle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{SendableValue, StreamItem};
    use crate::test_helpers::skip_if_no_python;
    use crossbeam_channel::bounded;
    use serial_test::serial;

    const PY_EVAL_INPUT: i64 = 258;
    const PY_FILE_INPUT: i64 = 257;

    fn make_globals(api: &PythonApi) -> *mut PyObject {
        let globals = api.dict_new();
        let builtins_key = api.string_from_str("__builtins__");
        let builtins = api
            .import_module("builtins")
            .expect("builtins should import");
        api.dict_set_item(globals, builtins_key, builtins);
        api.decref(builtins_key);
        api.decref(builtins);
        globals
    }

    fn run_file(api: &PythonApi, globals: *mut PyObject, code: &str) {
        let result = api
            .run_string(code, PY_FILE_INPUT, globals, globals)
            .expect("python file input should succeed");
        if !result.is_null() {
            api.decref(result);
        }
    }

    fn eval_obj(api: &PythonApi, globals: *mut PyObject, code: &str) -> *mut PyObject {
        let result = api
            .run_string(code, PY_EVAL_INPUT, globals, globals)
            .expect("python eval should succeed");
        assert!(!result.is_null());
        result
    }

    fn restore_new_event_loop(api: &PythonApi, globals: *mut PyObject) {
        run_file(
            api,
            globals,
            r#"
import asyncio
if "_saved_new_event_loop" in globals():
    asyncio.new_event_loop = _saved_new_event_loop
    del _saved_new_event_loop
"#,
        );
        if api.has_error() {
            api.clear_error();
        }
    }

    fn cleanup_globals(api: &PythonApi, globals: *mut PyObject) {
        if api.has_error() {
            api.clear_error();
        }
        api.decref(globals);
    }

    fn assert_single_error_contains(items: &[StreamItem], needle: &str) {
        assert_eq!(items.len(), 1, "expected one stream item");
        match &items[0] {
            StreamItem::Error(msg) => {
                assert!(
                    msg.contains(needle),
                    "expected error message to contain '{needle}', got '{msg}'"
                );
            }
            _ => panic!("expected StreamItem::Error"),
        }
    }

    fn assert_values_then_end(items: &[StreamItem], expected: &[i64]) {
        assert_eq!(items.len(), expected.len() + 1, "unexpected stream length");
        for (idx, expected_num) in expected.iter().enumerate() {
            match &items[idx] {
                StreamItem::Value(SendableValue::Integer(actual)) => {
                    assert_eq!(*actual, *expected_num, "unexpected value at index {idx}");
                }
                _ => panic!("expected integer value item"),
            }
        }
        match items.last() {
            Some(StreamItem::End) => {}
            _ => panic!("expected StreamItem::End as last item"),
        }
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_null_input() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);

        drive_async_generator(api, std::ptr::null_mut(), &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        assert_single_error_contains(&items, "Async generator is null");
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_yields_values_then_end() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _FakeAgen:
    def __init__(self, values):
        self._values = list(values)
        self._idx = 0
    def __anext__(self):
        if self._idx >= len(self._values):
            def _raise_stop():
                raise StopAsyncIteration
            return _raise_stop
        value = self._values[self._idx]
        self._idx += 1
        return lambda value=value: value
"#,
        );

        let async_gen = eval_obj(api, globals, "_FakeAgen([0, 1, 2])");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);

        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_values_then_end(&items, &[0, 1, 2]);

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_propagates_async_error() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _BoomAgen:
    def __init__(self):
        self._idx = 0
    def __anext__(self):
        if self._idx == 0:
            self._idx += 1
            return lambda: 1
        def _raise_boom():
            raise ValueError("async boom")
        return _raise_boom
"#,
        );

        let async_gen = eval_obj(api, globals, "_BoomAgen()");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);

        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_eq!(items.len(), 2, "expected one value then one error");
        match &items[0] {
            StreamItem::Value(SendableValue::Integer(v)) => assert_eq!(*v, 1),
            _ => panic!("expected first item to be integer value"),
        }
        match &items[1] {
            StreamItem::Error(msg) => {
                assert!(
                    msg.contains("ValueError") || msg.contains("async boom"),
                    "unexpected error message: {msg}"
                );
            }
            _ => panic!("expected second item to be error"),
        }

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_rejects_non_async_object() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        let sync_iter = eval_obj(api, globals, "iter(range(3))");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);

        drive_async_generator(api, sync_iter, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        assert_single_error_contains(&items, "__anext__");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_handles_immediate_anext_failure() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
class BrokenAsync:
    def __anext__(self):
        raise RuntimeError("anext failed")
"#,
        );

        let broken_obj = eval_obj(api, globals, "BrokenAsync()");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);

        drive_async_generator(api, broken_obj, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        assert_single_error_contains(&items, "RuntimeError");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_respects_cancel_signal() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _FakeAgen:
    def __anext__(self):
        return lambda: 1
"#,
        );

        let async_gen = eval_obj(api, globals, "_FakeAgen()");
        let (value_tx, value_rx) = unbounded();
        let (cancel_tx, cancel_rx) = bounded(1);
        cancel_tx.send(()).expect("cancel signal should send");

        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert!(items.is_empty(), "expected no output after cancellation");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_missing_new_event_loop_attr() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop
del asyncio.new_event_loop
"#,
        );

        let obj = eval_obj(api, globals, "iter(range(1))");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, obj, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_single_error_contains(&items, "asyncio.new_event_loop");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_event_loop_creation_failure() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop
def _broken_new_event_loop():
    raise RuntimeError("loop create failed")
asyncio.new_event_loop = _broken_new_event_loop
"#,
        );

        let obj = eval_obj(api, globals, "iter(range(1))");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, obj, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_single_error_contains(&items, "Failed to create event loop");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_missing_run_until_complete() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop
class _NoRunLoop:
    def close(self):
        pass
asyncio.new_event_loop = lambda: _NoRunLoop()
"#,
        );

        let obj = eval_obj(api, globals, "iter(range(1))");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, obj, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_single_error_contains(&items, "run_until_complete");

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_conversion_error() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _ObjAgen:
    def __anext__(self):
        return lambda: object()
"#,
        );

        let async_gen = eval_obj(api, globals, "_ObjAgen()");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_eq!(items.len(), 1, "expected a single conversion error");
        match &items[0] {
            StreamItem::Error(msg) => {
                assert!(
                    msg.contains("Cannot convert") || msg.contains("convert Python value"),
                    "unexpected conversion error message: {msg}"
                );
            }
            _ => panic!("expected error item for conversion failure"),
        }

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_close_failure_does_not_break_output() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _CloseFailLoop:
    def run_until_complete(self, coro):
        return coro()
    def close(self):
        raise RuntimeError("close failed")

asyncio.new_event_loop = _CloseFailLoop

class _FakeAgen:
    def __init__(self, values):
        self._values = list(values)
        self._idx = 0
    def __anext__(self):
        if self._idx >= len(self._values):
            def _raise_stop():
                raise StopAsyncIteration
            return _raise_stop
        value = self._values[self._idx]
        self._idx += 1
        return lambda value=value: value
"#,
        );

        let async_gen = eval_obj(api, globals, "_FakeAgen([0, 1])");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_values_then_end(&items, &[0, 1]);

        cleanup_globals(api, globals);
    }

    #[test]
    #[serial]
    fn test_drive_async_generator_missing_close_attr_does_not_break_output() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);

        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _NoCloseLoop:
    def run_until_complete(self, coro):
        return coro()

asyncio.new_event_loop = _NoCloseLoop

class _FakeAgen:
    def __init__(self, values):
        self._values = list(values)
        self._idx = 0
    def __anext__(self):
        if self._idx >= len(self._values):
            def _raise_stop():
                raise StopAsyncIteration
            return _raise_stop
        value = self._values[self._idx]
        self._idx += 1
        return lambda value=value: value
"#,
        );

        let async_gen = eval_obj(api, globals, "_FakeAgen([0])");
        let (value_tx, value_rx) = unbounded();
        let (_cancel_tx, cancel_rx) = bounded(1);
        drive_async_generator(api, async_gen, &value_tx, &cancel_rx);

        let items: Vec<StreamItem> = value_rx.try_iter().collect();
        restore_new_event_loop(api, globals);
        assert_values_then_end(&items, &[0]);

        cleanup_globals(api, globals);
    }

    // ── AsyncGeneratorStream integration tests ──────────────────────

    fn collect_stream(stream: &AsyncGeneratorStream) -> Vec<StreamItem> {
        let mut items = Vec::new();
        let timeout = std::time::Duration::from_secs(10);
        loop {
            match stream.receiver.recv_timeout(timeout) {
                Ok(item) => {
                    let done = matches!(&item, StreamItem::End | StreamItem::Error(_));
                    items.push(item);
                    if done {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    panic!("timeout waiting for stream item");
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
        items
    }

    #[test]
    #[serial]
    fn test_stream_from_sync_iterator_yields_values() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        let py_iter = eval_obj(api, globals, "iter(range(3))");
        // Drop GIL so the worker thread spawned by from_sync_iterator can acquire it
        drop(guard);

        let stream = AsyncGeneratorStream::from_sync_iterator(py_iter);
        let items = collect_stream(&stream);
        assert_values_then_end(&items, &[0, 1, 2]);

        let gil = api.ensure_gil();
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_sync_iterator_empty() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        let py_iter = eval_obj(api, globals, "iter(range(0))");
        drop(guard);

        let stream = AsyncGeneratorStream::from_sync_iterator(py_iter);
        let items = collect_stream(&stream);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], StreamItem::End));

        let gil = api.ensure_gil();
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_python_object_sync_path() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        // range(3) is iterable but not an async generator —
        // from_python_object should detect it as sync and use object_get_iter
        let py_obj = eval_obj(api, globals, "range(3)");
        drop(guard);

        let stream = AsyncGeneratorStream::from_python_object(py_obj, AsyncStrategy::PythonAdapter)
            .expect("should create stream from sync iterable");
        let items = collect_stream(&stream);
        assert_values_then_end(&items, &[0, 1, 2]);

        let gil = api.ensure_gil();
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_python_object_non_iterable_returns_error() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        let py_obj = eval_obj(api, globals, "42");
        drop(guard);

        let result = AsyncGeneratorStream::from_python_object(py_obj, AsyncStrategy::PythonAdapter);
        match result {
            Err(msg) => {
                assert!(
                    msg.contains("not iterable"),
                    "expected 'not iterable' error, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for non-iterable object"),
        }

        let gil = api.ensure_gil();
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_sync_iterator_propagates_error() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        run_file(
            api,
            globals,
            r#"
class _ErrorIter:
    def __init__(self):
        self._idx = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self._idx == 0:
            self._idx += 1
            return 1
        raise ValueError("sync boom")
"#,
        );
        let py_iter = eval_obj(api, globals, "_ErrorIter()");
        drop(guard);

        let stream = AsyncGeneratorStream::from_sync_iterator(py_iter);
        let items = collect_stream(&stream);
        assert_eq!(items.len(), 2, "expected one value then one error");
        match &items[0] {
            StreamItem::Value(SendableValue::Integer(v)) => assert_eq!(*v, 1),
            _ => panic!("expected first item to be integer value"),
        }
        match &items[1] {
            StreamItem::Error(msg) => {
                assert!(
                    msg.contains("ValueError") || msg.contains("sync boom"),
                    "unexpected error message: {msg}"
                );
            }
            _ => panic!("expected second item to be error"),
        }

        let gil = api.ensure_gil();
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_python_object_async_rust_driving() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _FakeAsyncGen:
    def __init__(self, values):
        self._values = list(values)
        self._idx = 0
    def __aiter__(self):
        return self
    def __anext__(self):
        if self._idx >= len(self._values):
            def _raise_stop():
                raise StopAsyncIteration
            return _raise_stop
        value = self._values[self._idx]
        self._idx += 1
        return lambda value=value: value
"#,
        );
        let async_gen = eval_obj(api, globals, "_FakeAsyncGen([0, 1, 2])");
        drop(guard);

        let stream =
            AsyncGeneratorStream::from_python_object(async_gen, AsyncStrategy::RustDriving)
                .expect("should create stream from async generator");
        let items = collect_stream(&stream);
        assert_values_then_end(&items, &[0, 1, 2]);

        let gil = api.ensure_gil();
        restore_new_event_loop(api, globals);
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_python_object_async_rust_driving_with_error() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _ErrorAsyncGen:
    def __init__(self):
        self._yielded = False
    def __aiter__(self):
        return self
    def __anext__(self):
        if not self._yielded:
            self._yielded = True
            return lambda: 1
        def _raise():
            raise ValueError("async gen error")
        return _raise
"#,
        );
        let async_gen = eval_obj(api, globals, "_ErrorAsyncGen()");
        drop(guard);

        let stream =
            AsyncGeneratorStream::from_python_object(async_gen, AsyncStrategy::RustDriving)
                .expect("should create stream");
        let items = collect_stream(&stream);
        assert_eq!(items.len(), 2, "expected one value then one error");
        match &items[0] {
            StreamItem::Value(SendableValue::Integer(v)) => assert_eq!(*v, 1),
            _ => panic!("expected first item to be integer value"),
        }
        match &items[1] {
            StreamItem::Error(msg) => {
                assert!(
                    msg.contains("ValueError") || msg.contains("async gen error"),
                    "unexpected error message: {msg}"
                );
            }
            _ => panic!("expected second item to be error"),
        }

        let gil = api.ensure_gil();
        restore_new_event_loop(api, globals);
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }

    #[test]
    #[serial]
    fn test_stream_from_python_object_async_rust_driving_empty() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();
        let globals = make_globals(api);
        run_file(
            api,
            globals,
            r#"
import asyncio
_saved_new_event_loop = asyncio.new_event_loop

class _DriveLoop:
    def run_until_complete(self, awaitable):
        return awaitable()
    def close(self):
        pass

asyncio.new_event_loop = _DriveLoop

class _EmptyAsyncGen:
    def __aiter__(self):
        return self
    def __anext__(self):
        def _raise_stop():
            raise StopAsyncIteration
        return _raise_stop
"#,
        );
        let async_gen = eval_obj(api, globals, "_EmptyAsyncGen()");
        drop(guard);

        let stream =
            AsyncGeneratorStream::from_python_object(async_gen, AsyncStrategy::RustDriving)
                .expect("should create stream from empty async generator");
        let items = collect_stream(&stream);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], StreamItem::End));

        let gil = api.ensure_gil();
        restore_new_event_loop(api, globals);
        cleanup_globals(api, globals);
        api.release_gil(gil);
    }
}
