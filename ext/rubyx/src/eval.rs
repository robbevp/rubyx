use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::python_guard::PyGuard;
use crate::ruby_helpers::runtime_error;
use crate::rubyx_object::{ruby_to_python, RubyxObject};
use magnus::r_hash::ForEach;
use magnus::typed_data::Obj;
use magnus::{Error, IntoValue, RHash, Ruby, TryConvert, Value};

/// Py_eval_input = 258 (for expressions)
const PY_EVAL_INPUT: i64 = 258;
/// Py_file_input = 257 (for statements)
const PY_FILE_INPUT: i64 = 257;

pub(crate) fn make_globals(api: &PythonApi) -> PyGuard<'_> {
    let globals = api.dict_new();
    // Import __builtins__ so expressions like len([1,2]) work
    let builtins_key = PyGuard::new(api.string_from_str("__builtins__"), api)
        .expect("string_from_str should not return null");
    let builtins = PyGuard::new(
        api.import_module("builtins")
            .expect("builtins should exist"),
        api,
    )
    .expect("builtins module should not be null");
    api.dict_set_item(globals, builtins_key.ptr(), builtins.ptr());
    // builtins_key and builtins are automatically decref'd here on drop
    PyGuard::new(globals, api).expect("dict_new should not return null")
}

/// Use Python's `ast` module to determine if the last statement in `code` is an
/// expression (`ast.Expr` node). If so, returns `Ok((body, last_expr))` where
/// `body` contains all preceding lines and `last_expr` is the expression source.
/// Returns `Err` if the last statement is not an expression or if parsing fails.
fn split_last_expr(
    api: &PythonApi,
    code: &str,
    globals: *mut crate::python_ffi::PyObject,
) -> Result<(String, String), String> {
    let key = PyGuard::new(api.string_from_str("__rubyx_src__"), api)
        .ok_or_else(|| "failed to create __rubyx_src__ key".to_string())?;
    let val = PyGuard::new(api.string_from_str(code), api)
        .ok_or_else(|| "failed to create code string".to_string())?;
    api.dict_set_item(globals, key.ptr(), val.ptr());

    // Parse with ast.parse() and check if body[-1] is an ast.Expr node.
    // Returns the 1-based line number of the last expression, or -1.
    let ast_expr = "(lambda a: (lambda t: t.body[-1].lineno \
                     if t.body and isinstance(t.body[-1], a.Expr) \
                     else -1)(a.parse(__rubyx_src__)))(__import__('ast'))";

    let result = api
        .run_string(ast_expr, PY_EVAL_INPUT, globals, globals)
        .map_err(|e| format!("AST parse failed: {e}"))?;

    if result.is_null() {
        if api.has_error() {
            PythonApi::extract_exception(api);
        }
        return Err("AST parse returned null".to_string());
    }

    let result_guard =
        PyGuard::new(result, api).ok_or_else(|| "AST result was null".to_string())?;
    let lineno = api.long_to_i64(result_guard.ptr());

    if lineno < 1 {
        return Err("last statement is not an expression".to_string());
    }

    let lines: Vec<&str> = code.lines().collect();
    let split_idx = (lineno as usize) - 1; // Convert to 0-based

    if split_idx == 0 {
        return Err("expression is the entire code".to_string());
    }

    let body = lines[..split_idx].join("\n");
    let last_expr = lines[split_idx..].join("\n");

    Ok((body, last_expr))
}

pub(crate) fn eval_with_globals(
    code: &str,
    globals: *mut PyObject,
    api: &'static PythonApi,
) -> Result<Value, magnus::Error> {
    let ruby = Ruby::get()
        .map_err(|e| Error::new(runtime_error(), format!("Ruby VM unavailable: {e}")))?;
    // Try as expression first (Py_eval_input)
    let py_result = match api.run_string(code, PY_EVAL_INPUT, globals, globals) {
        Ok(output) if !output.is_null() => output,
        Ok(_) => {
            // Expression eval failed — extract the exception to check type.
            // extract_exception consumes the error, so we must save it.
            let exc = if api.has_error() {
                PythonApi::extract_exception(api)
            } else {
                None
            };

            let is_syntax = matches!(
                exc,
                Some(crate::exception::PythonException::SyntaxError { .. })
            );

            if !is_syntax {
                // Real error (NameError, KeyError, etc.) — not a syntax issue
                let err = exc
                    .map(Error::from)
                    .unwrap_or_else(|| Error::new(runtime_error(), "Python execution failed"));
                return Err(err);
            }

            // SyntaxError — code contains statements. Use AST to split.
            let trimmed = code.trim_end();

            match split_last_expr(api, trimmed, globals) {
                Ok((body, last_expr)) => {
                    // Run body (statements) with Py_file_input
                    match api.run_string(&body, PY_FILE_INPUT, globals, globals) {
                        Ok(out) if out.is_null() => {
                            let err = PythonApi::extract_exception(api)
                                .map(Error::from)
                                .unwrap_or_else(|| {
                                    Error::new(runtime_error(), "Python execution failed")
                                });
                            return Err(err);
                        }
                        Ok(_) => { /* Py_file_input returns Py_None — ignore */ }
                        Err(e) => {
                            return Err(Error::new(runtime_error(), e));
                        }
                    }

                    // Eval the last expression to get its value
                    match api.run_string(&last_expr, PY_EVAL_INPUT, globals, globals) {
                        Ok(out) if !out.is_null() => out,
                        Ok(_) => {
                            // AST said it's an expression but eval failed — shouldn't happen
                            if api.has_error() {
                                let err = PythonApi::extract_exception(api)
                                    .map(Error::from)
                                    .unwrap_or_else(|| {
                                        Error::new(runtime_error(), "Python execution failed")
                                    });
                                return Err(err);
                            }
                            return Err(Error::new(runtime_error(), "Python execution failed"));
                        }
                        Err(e) => {
                            return Err(Error::new(runtime_error(), e));
                        }
                    }
                }
                Err(_) => {
                    // Last statement is not an expression (or AST parse failed)
                    // — run entire code as statements
                    match api.run_string(trimmed, PY_FILE_INPUT, globals, globals) {
                        Ok(out) if out.is_null() => {
                            let err = PythonApi::extract_exception(api)
                                .map(Error::from)
                                .unwrap_or_else(|| {
                                    Error::new(runtime_error(), "Python execution failed")
                                });
                            return Err(err);
                        }
                        Ok(out) => out, // Py_None — no expression value to return
                        Err(e) => {
                            return Err(Error::new(runtime_error(), e));
                        }
                    }
                }
            }
        }
        Err(e) => {
            return Err(Error::new(runtime_error(), e));
        }
    };

    let py_result_guard = PyGuard::new(py_result, api)
        .ok_or_else(|| Error::new(runtime_error(), "Python returned null result"))?;

    // Wrap result — RubyxObject::new increfs, so we decref our reference after
    let wrapper = RubyxObject::new(py_result_guard.ptr(), api)
        .ok_or_else(|| Error::new(runtime_error(), "Failed to create RubyxObject"))?;
    Ok(wrapper.into_value_with(&ruby))
}

pub(crate) fn rubyx_eval(code: String) -> Result<Value, magnus::Error> {
    let api = crate::api();
    let gil = api.ensure_gil();

    let result = {
        let globals = make_globals(api);
        eval_with_globals(&code, globals.ptr(), api)
    };

    api.release_gil(gil);
    result
}
pub(crate) fn rubyx_eval_with_globals(
    code: String,
    globals_hash: RHash,
) -> Result<Value, magnus::Error> {
    let api = crate::api();
    let gil = api.ensure_gil();

    let globals = make_globals(api);
    let result = match inject_globals(&globals, globals_hash, api) {
        Ok(()) => eval_with_globals(&code, globals.ptr(), api),
        Err(e) => Err(e),
    };
    drop(globals); // decref while GIL is held

    api.release_gil(gil);
    result
}

fn inject_globals(
    globals: &PyGuard<'_>,
    globals_hash: RHash,
    api: &'static PythonApi,
) -> Result<(), magnus::Error> {
    globals_hash.foreach(|key: Value, val: Value| {
        let py_key = ruby_to_python(key, api)?;
        let py_val = ruby_to_python(val, api)?;
        api.dict_set_item(globals.ptr(), py_key, py_val);
        api.decref(py_key);
        api.decref(py_val);
        Ok(ForEach::Continue)
    })?;
    Ok(())
}

/// Run a Python coroutine with asyncio.run() and return the result.
/// The coroutine must already be a PyObject (not code string).
/// Caller must hold the GIL.
fn run_asyncio(coroutine: *mut PyObject, api: &'static PythonApi) -> Result<Value, magnus::Error> {
    let ruby = Ruby::get().map_err(|e| Error::new(runtime_error(), e.to_string()))?;

    let asyncio = api
        .import_module("asyncio")
        .map_err(|e| Error::new(runtime_error(), e.to_string()))?;
    let run_fn = api.object_get_attr_string(asyncio, "run");

    if run_fn.is_null() {
        api.clear_error();
        api.decref(asyncio);
        return Err(Error::new(runtime_error(), "asyncio.run not found"));
    }

    let args = unsafe { (api.py_tuple_new)(1) };
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
        return Err(Error::new(runtime_error(), err));
    }

    let wrapper = RubyxObject::new(result, api)
        .ok_or_else(|| Error::new(runtime_error(), "Failed to wrap async result"))?;

    Ok(wrapper.into_value_with(&ruby))
}

/// Rubyx.await(coroutine) — takes a RubyxObject wrapping a Python coroutine,
/// runs it with asyncio.run(), and returns the result.
pub(crate) fn rubyx_await(coroutine: Value) -> Result<Value, magnus::Error> {
    let obj = Obj::<RubyxObject>::try_convert(coroutine)?;
    let api = crate::api();
    let gil = api.ensure_gil();

    let result = run_asyncio(obj.as_ptr(), api);

    api.release_gil(gil);
    result
}

/// Eval code in context globals to get a coroutine, then run it with asyncio.run().
/// Used by RubyxContext#await.
pub(crate) fn await_eval_with_globals(
    code: &str,
    globals: *mut PyObject,
    api: &'static PythonApi,
) -> Result<Value, magnus::Error> {
    let py_coroutine = match api.run_string(code, PY_EVAL_INPUT, globals, globals) {
        Ok(obj) if !obj.is_null() => obj,
        Ok(_) => {
            let err = if api.has_error() {
                PythonApi::extract_exception(api)
                    .map(Error::from)
                    .unwrap_or_else(|| Error::new(runtime_error(), "Python eval failed"))
            } else {
                Error::new(runtime_error(), "Python eval returned null")
            };
            return Err(err);
        }
        Err(e) => return Err(Error::new(runtime_error(), e)),
    };

    let result = run_asyncio(py_coroutine, api);
    api.decref(py_coroutine);
    result
}

pub(crate) fn rubyx_await_with_globals(
    code: String,
    globals_hash: RHash,
) -> Result<Value, magnus::Error> {
    let api = crate::api();
    let gil = api.ensure_gil();

    let globals = make_globals(api);
    let result = match inject_globals(&globals, globals_hash, api) {
        Ok(()) => await_eval_with_globals(&code, globals.ptr(), api),
        Err(e) => Err(e),
    };
    drop(globals);

    api.release_gil(gil);
    result
}

pub(crate) fn rubyx_async_await_with_globals(
    code: String,
    globals_hash: RHash,
) -> Result<crate::future::RubyxFuture, magnus::Error> {
    let api = crate::api();
    let gil = api.ensure_gil();

    let globals = make_globals(api);
    let result = match inject_globals(&globals, globals_hash, api) {
        Err(e) => Err(e),
        Ok(()) => {
            match api.run_string(&code, PY_EVAL_INPUT, globals.ptr(), globals.ptr()) {
                Ok(obj) if !obj.is_null() => {
                    let future = crate::future::RubyxFuture::from_coroutine(obj, api);
                    api.decref(obj);
                    Ok(future)
                }
                Ok(_) => {
                    let err = if api.has_error() {
                        PythonApi::extract_exception(api)
                            .map(Error::from)
                            .unwrap_or_else(|| Error::new(runtime_error(), "Python eval failed"))
                    } else {
                        Error::new(runtime_error(), "Python eval returned null")
                    };
                    Err(err)
                }
                Err(e) => Err(Error::new(runtime_error(), e)),
            }
        }
    };
    drop(globals);

    api.release_gil(gil);
    result
}
