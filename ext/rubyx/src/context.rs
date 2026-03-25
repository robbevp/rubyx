use crate::eval::{await_eval_with_globals, eval_with_globals, make_globals};
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::rubyx_object::ruby_to_python;
use magnus::r_hash::ForEach;
use magnus::{RHash, Value};

#[magnus::wrap(class = "Rubyx::Context", free_immediately)]
pub(crate) struct RubyxContext {
    globals: *mut PyObject,
    api: &'static PythonApi,
}

unsafe impl Send for RubyxContext {}
unsafe impl Sync for RubyxContext {}

impl RubyxContext {
    pub(crate) fn new() -> Result<Self, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();

        let guard = make_globals(api);
        let globals = guard.ptr();
        api.incref(globals);

        api.release_gil(gil);
        Ok(Self { globals, api })
    }

    pub(crate) fn eval(&self, code: String) -> Result<magnus::Value, magnus::Error> {
        let gil = self.api.ensure_gil();
        let result = eval_with_globals(&code, self.globals, self.api);
        self.api.release_gil(gil);
        result
    }

    pub(crate) fn eval_with_globals(
        &self,
        code: String,
        globals_hash: RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        let gil = self.api.ensure_gil();
        let result = match self.inject_globals(globals_hash) {
            Ok(()) => eval_with_globals(&code, self.globals, self.api),
            Err(e) => Err(e),
        };
        self.api.release_gil(gil);
        result
    }

    pub(crate) fn await_eval(&self, code: String) -> Result<magnus::Value, magnus::Error> {
        let gil = self.api.ensure_gil();
        let future = await_eval_with_globals(&code, self.globals, self.api);
        self.api.release_gil(gil);
        match future {
            Ok(value) => Ok(value.value()?),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn await_eval_with_globals(
        &self,
        code: String,
        globals_hash: RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        let gil = self.api.ensure_gil();
        let future = match self.inject_globals(globals_hash) {
            Ok(()) => await_eval_with_globals(&code, self.globals, self.api),
            Err(e) => Err(e),
        };
        self.api.release_gil(gil);
        match future {
            Ok(value) => Ok(value.value()?),
            Err(e) => Err(e),
        }
    }

    /// Eval code to get a coroutine, then run it on a background thread.
    /// Returns a Rubyx::Future immediately.
    pub(crate) fn async_await_eval(
        &self,
        code: String,
    ) -> Result<crate::future::RubyxFuture, magnus::Error> {
        let gil = self.api.ensure_gil();

        // Eval the code in context globals to get the coroutine
        let py_coroutine = match self.api.run_string(&code, 258, self.globals, self.globals) {
            Ok(obj) if !obj.is_null() => obj,
            Ok(_) => {
                let err = if self.api.has_error() {
                    crate::python_api::PythonApi::extract_exception(self.api)
                        .map(magnus::Error::from)
                        .unwrap_or_else(|| {
                            magnus::Error::new(
                                crate::ruby_helpers::runtime_error(),
                                "Python eval failed",
                            )
                        })
                } else {
                    magnus::Error::new(
                        crate::ruby_helpers::runtime_error(),
                        "Python eval returned null",
                    )
                };
                self.api.release_gil(gil);
                return Err(err);
            }
            Err(e) => {
                self.api.release_gil(gil);
                return Err(magnus::Error::new(crate::ruby_helpers::runtime_error(), e));
            }
        };

        let future = crate::future::RubyxFuture::from_coroutine(py_coroutine, self.api);
        self.api.decref(py_coroutine);
        self.api.release_gil(gil);

        Ok(future)
    }

    pub(crate) fn async_await_eval_with_globals(
        &self,
        code: String,
        globals_hash: RHash,
    ) -> Result<crate::future::RubyxFuture, magnus::Error> {
        let gil = self.api.ensure_gil();

        if let Err(e) = self.inject_globals(globals_hash) {
            self.api.release_gil(gil);
            return Err(e);
        }

        let py_coroutine = match self.api.run_string(&code, 258, self.globals, self.globals) {
            Ok(obj) if !obj.is_null() => obj,
            Ok(_) => {
                let err = if self.api.has_error() {
                    crate::python_api::PythonApi::extract_exception(self.api)
                        .map(magnus::Error::from)
                        .unwrap_or_else(|| {
                            magnus::Error::new(
                                crate::ruby_helpers::runtime_error(),
                                "Python eval failed",
                            )
                        })
                } else {
                    magnus::Error::new(
                        crate::ruby_helpers::runtime_error(),
                        "Python eval returned null",
                    )
                };
                self.api.release_gil(gil);
                return Err(err);
            }
            Err(e) => {
                self.api.release_gil(gil);
                return Err(magnus::Error::new(crate::ruby_helpers::runtime_error(), e));
            }
        };

        let future = crate::future::RubyxFuture::from_coroutine(py_coroutine, self.api);
        self.api.decref(py_coroutine);
        self.api.release_gil(gil);

        Ok(future)
    }

    /// Merge a Ruby Hash into the persistent globals dict.
    /// Caller must hold the GIL.
    fn inject_globals(&self, globals_hash: RHash) -> Result<(), magnus::Error> {
        let api = self.api;
        let globals = self.globals;
        let mut err: Option<magnus::Error> = None;
        globals_hash.foreach(|key: Value, val: Value| {
            let py_key = match ruby_to_python(key, api) {
                Ok(k) => k,
                Err(e) => {
                    err = Some(e);
                    return Ok(ForEach::Stop);
                }
            };
            let py_val = match ruby_to_python(val, api) {
                Ok(v) => v,
                Err(e) => {
                    api.decref(py_key);
                    err = Some(e);
                    return Ok(ForEach::Stop);
                }
            };
            api.dict_set_item(globals, py_key, py_val);
            api.decref(py_key);
            api.decref(py_val);
            Ok(ForEach::Continue)
        })?;
        if let Some(e) = err {
            return Err(e);
        }
        Ok(())
    }
}

impl Drop for RubyxContext {
    fn drop(&mut self) {
        if self.globals.is_null() {
            return;
        }
        if !self.api.is_initialized() {
            return;
        }
        let gil = self.api.ensure_gil();
        self.api.decref(self.globals);
        self.api.release_gil(gil);
    }
}

#[cfg(test)]
mod tests {
    use crate::eval::make_globals;
    use crate::test_helpers::skip_if_no_python;
    use serial_test::serial;

    // ========== Construction & Globals Lifecycle ==========

    #[test]
    #[serial]
    fn test_make_globals_and_incref_keeps_dict_alive() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // incref so the dict survives the PyGuard drop
        api.incref(globals);
        drop(globals_guard); // decrefs once — refcount should be 1

        // dict should still be usable
        let size = api.dict_size(globals);
        assert!(size >= 1, "globals should have at least __builtins__");

        // cleanup
        api.decref(globals);
    }

    #[test]
    #[serial]
    fn test_globals_has_builtins() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        let key = api.string_from_str("__builtins__");
        assert!(!key.is_null());
        let builtins = api.dict_get_item(globals, key);
        assert!(!builtins.is_null(), "globals should contain __builtins__");

        api.decref(key);
    }

    // ========== eval_with_globals: State Persistence ==========

    #[test]
    #[serial]
    fn test_eval_with_globals_state_persists() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // Set a variable
        api.run_simple_string("x = 42").ok(); // this uses its own globals
                                              // Instead, use run_string with our globals
        let set_result = api.run_string("x = 42", 257, globals, globals);
        assert!(set_result.is_ok(), "setting x = 42 should succeed");

        // Read it back from the same globals
        let get_result = api.run_string("x", 258, globals, globals);
        assert!(get_result.is_ok(), "reading x should succeed");
        let py_obj = get_result.unwrap();
        assert!(!py_obj.is_null());

        let value = api.long_to_i64(py_obj);
        assert_eq!(value, 42);
        api.decref(py_obj);
    }

    #[test]
    #[serial]
    fn test_eval_with_globals_accumulates_state() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // Multiple assignments accumulate
        let _ = api.run_string("a = 10", 257, globals, globals);
        let _ = api.run_string("b = 20", 257, globals, globals);
        let _ = api.run_string("c = a + b", 257, globals, globals);

        let result = api.run_string("c", 258, globals, globals);
        assert!(result.is_ok());
        let py_obj = result.unwrap();
        assert!(!py_obj.is_null());
        assert_eq!(api.long_to_i64(py_obj), 30);
        api.decref(py_obj);
    }

    #[test]
    #[serial]
    fn test_eval_with_globals_functions_persist() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // Define a function
        let _ = api.run_string("def double(n): return n * 2", 257, globals, globals);

        // Call it
        let result = api.run_string("double(21)", 258, globals, globals);
        assert!(result.is_ok());
        let py_obj = result.unwrap();
        assert!(!py_obj.is_null());
        assert_eq!(api.long_to_i64(py_obj), 42);
        api.decref(py_obj);
    }

    #[test]
    #[serial]
    fn test_eval_with_globals_imports_persist() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // Import a module
        let _ = api.run_string("import math", 257, globals, globals);

        // Use it
        let result = api.run_string("math.factorial(5)", 258, globals, globals);
        assert!(result.is_ok());
        let py_obj = result.unwrap();
        assert!(!py_obj.is_null());
        assert_eq!(api.long_to_i64(py_obj), 120);
        api.decref(py_obj);
    }

    // ========== Isolation Between Globals Dicts ==========

    #[test]
    #[serial]
    fn test_separate_globals_are_isolated() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals1 = make_globals(api);
        let globals2 = make_globals(api);

        // Set variable in globals1
        let _ = api.run_string("isolated_var = 999", 257, globals1.ptr(), globals1.ptr());

        // Should NOT be visible in globals2
        let result = api.run_string("isolated_var", 258, globals2.ptr(), globals2.ptr());
        // This should fail (NameError) or return null
        match result {
            Ok(obj) if obj.is_null() => {
                // Expected: Python set an error
                if api.has_error() {
                    crate::python_api::PythonApi::extract_exception(api);
                }
            }
            Ok(_obj) => {
                panic!("isolated_var should NOT be visible in a separate globals dict");
            }
            Err(_) => {
                // Also expected — run_string returned an error
            }
        }
    }

    // ========== Error Recovery ==========

    #[test]
    #[serial]
    fn test_error_does_not_corrupt_globals() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();

        // Set a variable
        let _ = api.run_string("x = 10", 257, globals, globals);

        // Cause an error
        let err_result = api.run_string("1 / 0", 258, globals, globals);
        match err_result {
            Ok(obj) if obj.is_null() => {
                if api.has_error() {
                    crate::python_api::PythonApi::extract_exception(api);
                }
            }
            Err(_) => {}
            _ => {}
        }

        // x should still be accessible
        let result = api.run_string("x", 258, globals, globals);
        assert!(result.is_ok());
        let py_obj = result.unwrap();
        assert!(!py_obj.is_null());
        assert_eq!(api.long_to_i64(py_obj), 10);
        api.decref(py_obj);
    }

    // ========== Drop Safety ==========

    #[test]
    #[serial]
    fn test_drop_with_null_globals_does_not_crash() {
        let Some(_guard) = skip_if_no_python() else {
            return;
        };
        let api = crate::api();

        // Manually construct with null globals to test the guard in Drop
        let ctx = super::RubyxContext {
            globals: std::ptr::null_mut(),
            api,
        };
        drop(ctx); // Should not crash
    }

    #[test]
    #[serial]
    fn test_drop_decrefs_globals() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        // Create globals with an extra incref (simulating what new() does)
        let globals_guard = make_globals(api);
        let globals = globals_guard.ptr();
        api.incref(globals); // refcount = 2
        drop(globals_guard); // refcount = 1

        // incref again so we can observe the decref from Drop
        api.incref(globals); // refcount = 2

        let ctx = super::RubyxContext { globals, api };
        drop(ctx); // Drop calls decref → refcount = 1

        // globals should still be valid (refcount = 1, our extra ref)
        let size = api.dict_size(globals);
        assert!(
            size >= 1,
            "globals should still be alive after context drop"
        );

        // Final cleanup
        api.decref(globals);
    }

    // ========== Original Eval Isolation ==========

    #[test]
    #[serial]
    fn test_rubyx_eval_still_isolated() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        // Create two separate globals — each should be independent
        let g1 = make_globals(api);
        let g2 = make_globals(api);

        let _ = api.run_string("leak_test = 123", 257, g1.ptr(), g1.ptr());

        // leak_test should not be in g2
        let result = api.run_string("leak_test", 258, g2.ptr(), g2.ptr());
        let leaked = match result {
            Ok(obj) if !obj.is_null() => {
                api.decref(obj);
                true
            }
            _ => {
                if api.has_error() {
                    crate::python_api::PythonApi::extract_exception(api);
                }
                false
            }
        };
        assert!(
            !leaked,
            "state should not leak between separate globals dicts"
        );
    }

    // ========== Multiple Contexts ==========

    #[test]
    #[serial]
    fn test_multiple_globals_independent_values() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let g1 = make_globals(api);
        let g2 = make_globals(api);
        let g3 = make_globals(api);

        let _ = api.run_string("val = 0", 257, g1.ptr(), g1.ptr());
        let _ = api.run_string("val = 10", 257, g2.ptr(), g2.ptr());
        let _ = api.run_string("val = 20", 257, g3.ptr(), g3.ptr());

        let r1 = api.run_string("val", 258, g1.ptr(), g1.ptr()).unwrap();
        let r2 = api.run_string("val", 258, g2.ptr(), g2.ptr()).unwrap();
        let r3 = api.run_string("val", 258, g3.ptr(), g3.ptr()).unwrap();

        assert_eq!(api.long_to_i64(r1), 0);
        assert_eq!(api.long_to_i64(r2), 10);
        assert_eq!(api.long_to_i64(r3), 20);

        api.decref(r1);
        api.decref(r2);
        api.decref(r3);
    }

    // ========== Context inject_globals tests ==========

    #[test]
    #[serial]
    fn test_context_inject_globals_simple() {
        use crate::test_helpers::with_ruby_python;
        use magnus::IntoValue;
        with_ruby_python(|ruby, api| {
            let globals_guard = make_globals(api);
            let globals = globals_guard.ptr();

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("x"), 10_i64.into_value_with(ruby))
                .unwrap();
            hash.aset(ruby.sym_new("y"), 20_i64.into_value_with(ruby))
                .unwrap();

            // Inject into globals
            let ctx = super::RubyxContext { globals, api };
            ctx.inject_globals(hash).expect("inject should succeed");

            // Verify x and y are in globals
            let key_x = api.string_from_str("x");
            let val_x = api.dict_get_item(globals, key_x);
            assert!(!val_x.is_null());
            assert_eq!(api.long_to_i64(val_x), 10);
            api.decref(key_x);

            let key_y = api.string_from_str("y");
            let val_y = api.dict_get_item(globals, key_y);
            assert!(!val_y.is_null());
            assert_eq!(api.long_to_i64(val_y), 20);
            api.decref(key_y);

            // Prevent Drop from double-decref
            std::mem::forget(ctx);
        });
    }

    #[test]
    #[serial]
    fn test_context_eval_with_globals() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("a"), 5_i64.into_value_with(ruby))
                .unwrap();
            hash.aset(ruby.sym_new("b"), 7_i64.into_value_with(ruby))
                .unwrap();

            let result = ctx
                .eval_with_globals("a * b".to_string(), hash)
                .expect("eval should succeed");

            let obj =
                magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(result)
                    .expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 35);
        });
    }

    #[test]
    #[serial]
    fn test_context_globals_persist_after_inject() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            // Inject x=100
            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("x"), 100_i64.into_value_with(ruby))
                .unwrap();
            let _ = ctx
                .eval_with_globals("y = x + 1".to_string(), hash)
                .expect("eval should succeed");

            // x and y should persist in context without re-injecting
            let result = ctx
                .eval("x + y".to_string())
                .expect("should access persisted globals");
            let obj =
                magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(result)
                    .expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 201); // 100 + 101
        });
    }

    #[test]
    #[serial]
    fn test_context_eval_with_globals_string_values() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("greeting"), "hello".into_value_with(ruby))
                .unwrap();
            hash.aset(ruby.sym_new("name"), "world".into_value_with(ruby))
                .unwrap();

            let result = ctx
                .eval_with_globals("f'{greeting}, {name}!'".to_string(), hash)
                .expect("eval should succeed");

            let obj =
                magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(result)
                    .expect("should be RubyxObject");
            assert_eq!(
                api.string_to_string(obj.as_ptr()),
                Some("hello, world!".to_string())
            );
        });
    }

    #[test]
    #[serial]
    fn test_context_eval_with_globals_list() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            let arr = magnus::RArray::new();
            arr.push(1_i64.into_value_with(ruby)).unwrap();
            arr.push(2_i64.into_value_with(ruby)).unwrap();
            arr.push(3_i64.into_value_with(ruby)).unwrap();

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("items"), arr.into_value_with(ruby))
                .unwrap();

            let result = ctx
                .eval_with_globals("sum(items)".to_string(), hash)
                .expect("eval should succeed");

            let obj =
                magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(result)
                    .expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 6);
        });
    }

    #[test]
    #[serial]
    fn test_context_await_with_globals() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            // Define async function in context
            ctx.eval("import asyncio\nasync def multiply(a, b): return a * b".to_string())
                .expect("should define function");

            // Inject globals into context
            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("a"), 6_i64.into_value_with(ruby))
                .unwrap();
            hash.aset(ruby.sym_new("b"), 7_i64.into_value_with(ruby))
                .unwrap();
            ctx.inject_globals(hash).expect("inject should succeed");

            // Manually create future (avoid ctx.await_eval_with_globals which
            // nests ensure_gil/release_gil incorrectly with with_ruby_python's GIL)
            let future = crate::eval::await_eval_with_globals(
                "multiply(a, b)",
                ctx.globals,
                api,
            )
            .expect("should create future");

            let tstate = api.save_thread();
            let result = future.value().expect("await should succeed");
            drop(future);
            api.restore_thread(tstate);

            assert_eq!(i64::try_convert(result).unwrap(), 42);
        });
    }

    #[test]
    #[serial]
    fn test_context_await_with_globals_error() {
        use crate::test_helpers::with_ruby_python;
        use magnus::IntoValue;
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            ctx.eval(
                "import asyncio\nasync def fail_if_neg(n):\n    if n < 0: raise ValueError('neg')\n    return n"
                    .to_string(),
            )
            .expect("should define function");

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("n"), (-5_i64).into_value_with(ruby))
                .unwrap();
            ctx.inject_globals(hash).expect("inject should succeed");

            let future_result = crate::eval::await_eval_with_globals(
                "fail_if_neg(n)",
                ctx.globals,
                api,
            );

            match future_result {
                Err(_) => {} // eval itself failed
                Ok(future) => {
                    let tstate = api.save_thread();
                    let result = future.value();
                    api.restore_thread(tstate);
                    assert!(result.is_err(), "should propagate ValueError");
                }
            }
        });
    }

    #[test]
    #[serial]
    fn test_context_async_await_with_globals() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            ctx.eval("import asyncio\nasync def add(x, y): return x + y".to_string())
                .expect("should define function");

            let hash = magnus::RHash::new();
            hash.aset(ruby.sym_new("x"), 15_i64.into_value_with(ruby))
                .unwrap();
            hash.aset(ruby.sym_new("y"), 27_i64.into_value_with(ruby))
                .unwrap();

            // Need to release GIL for the background thread
            let gil = api.ensure_gil();
            let future = ctx
                .async_await_eval_with_globals("add(x, y)".to_string(), hash)
                .expect("async_await should succeed");
            api.release_gil(gil);

            let tstate = api.save_thread();
            let result = future.value().expect("future should resolve");
            drop(future);
            api.restore_thread(tstate);

            assert_eq!(i64::try_convert(result).unwrap(), 42);
        });
    }

    #[test]
    #[serial]
    fn test_context_globals_override() {
        use crate::test_helpers::with_ruby_python;
        use magnus::{IntoValue, TryConvert};
        with_ruby_python(|ruby, api| {
            let ctx = super::RubyxContext::new().expect("context should create");

            // Inject x=10
            let hash1 = magnus::RHash::new();
            hash1
                .aset(ruby.sym_new("x"), 10_i64.into_value_with(ruby))
                .unwrap();
            let r1 = ctx
                .eval_with_globals("x".to_string(), hash1)
                .expect("eval should succeed");
            let obj1 = magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(r1)
                .unwrap();
            assert_eq!(api.long_to_i64(obj1.as_ptr()), 10);

            // Override x=99
            let hash2 = magnus::RHash::new();
            hash2
                .aset(ruby.sym_new("x"), 99_i64.into_value_with(ruby))
                .unwrap();
            let r2 = ctx
                .eval_with_globals("x".to_string(), hash2)
                .expect("eval should succeed");
            let obj2 = magnus::typed_data::Obj::<crate::rubyx_object::RubyxObject>::try_convert(r2)
                .unwrap();
            assert_eq!(api.long_to_i64(obj2.as_ptr()), 99);
        });
    }
}
