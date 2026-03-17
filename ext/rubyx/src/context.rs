use crate::eval::{eval_with_globals, make_globals};
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;

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
        assert!(size >= 1, "globals should still be alive after context drop");

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
        assert!(!leaked, "state should not leak between separate globals dicts");
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
}