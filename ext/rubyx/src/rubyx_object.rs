use crate::convert::ToPython;
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use crate::python_guard::PyGuard;
use crate::ruby_helpers;
use crate::stream::SendableValue;
use magnus::r_hash::ForEach;
use magnus::typed_data::Obj;
use magnus::value::ReprValue;
use magnus::{IntoValue, RHash, Ruby, Symbol, TryConvert, Value};
use std::ffi::CString;

const RUBY_IMPLICIT_CONVERSIONS: &[&str] = &[
    "to_ary",
    "to_str",
    "to_hash",
    "to_int",
    "to_float",
    "to_io",
    "to_proc",
    "to_path",
    "to_regexp",
];

pub(crate) fn python_to_sendable(
    py_val: *mut PyObject,
    api: &PythonApi,
) -> Result<SendableValue, String> {
    // Nil
    if py_val == api.py_none {
        return Ok(SendableValue::Nil);
    }
    // Bool must be checked before long, because Python bool is a subclass of int
    if api.is_bool(py_val) {
        return Ok(SendableValue::Bool(py_val == api.py_true));
    }
    if api.is_long(py_val) {
        let val = api.long_to_i64(py_val);
        return Ok(SendableValue::Integer(val));
    }
    if api.is_float(py_val) {
        let val = api.float_to_f64(py_val);
        return Ok(SendableValue::Float(val));
    }
    if api.is_string(py_val) {
        let Some(val) = api.string_to_string(py_val) else {
            if api.has_error() {
                api.clear_error();
            }
            return Err("Cannot decode Python string as UTF-8".to_string());
        };
        return Ok(SendableValue::Str(val));
    }
    if api.tuple_check(py_val) {
        let len = api.tuple_size(py_val);
        let mut items = Vec::with_capacity(len as usize);
        for i in 0..len {
            let item = api.tuple_get_item(py_val, i);
            items.push(python_to_sendable(item, api)?);
        }
        return Ok(SendableValue::List(items));
    }

    if api.list_check(py_val) {
        let len = api.list_size(py_val);
        let mut items = Vec::with_capacity(len as usize);
        for i in 0..len {
            let item = api.list_get_item(py_val, i);
            items.push(python_to_sendable(item, api)?);
        }
        return Ok(SendableValue::List(items));
    }

    if api.dict_check(py_val) {
        let len = api.dict_size(py_val);
        let mut items = Vec::with_capacity(len);
        let mut start = 0;
        let mut key = std::ptr::null_mut();
        let mut value = std::ptr::null_mut();
        while api.dict_next(py_val, &mut start, &mut key, &mut value) {
            let send_key = python_to_sendable(key, api)?;
            let send_value = python_to_sendable(value, api)?;
            items.push((send_key, send_value));
        }
        return Ok(SendableValue::Dict(items));
    }

    if py_val == api.py_true {
        return Ok(SendableValue::Bool(true));
    }
    if py_val == api.py_false {
        return Ok(SendableValue::Bool(false));
    }
    Err("Cannot convert Python value to Ruby".to_string())
}
fn ruby_to_python(value: Value, api: &PythonApi) -> Result<*mut PyObject, magnus::Error> {
    let ruby = Ruby::get().map_err(|e| {
        magnus::Error::new(
            ruby_helpers::runtime_error(),
            format!("Ruby VM handle unavailable: {e}"),
        )
    })?;
    if value.is_nil() {
        api.incref(api.py_none);
        return Ok(api.py_none);
    }
    if value.is_kind_of(ruby.class_true_class()) {
        api.incref(api.py_true);
        return Ok(api.py_true);
    }
    if value.is_kind_of(ruby.class_false_class()) {
        api.incref(api.py_false);
        return Ok(api.py_false);
    }
    if value.is_kind_of(ruby.class_integer()) {
        let val = i64::try_convert(value)?;
        return val
            .to_python(api)
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e.to_string()));
    }
    if value.is_kind_of(ruby.class_float()) {
        let val = f64::try_convert(value)?;
        return val
            .to_python(api)
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e.to_string()));
    }
    if value.is_kind_of(ruby.class_string()) {
        let val = String::try_convert(value)?;
        return val
            .to_python(api)
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e.to_string()));
    }
    // Already wrapped Python object
    if let Ok(obj) = Obj::<RubyxObject>::try_convert(value) {
        api.incref(obj.as_ptr()); // Obj<T> derefs to &T, so your as_ptr() works
        return Ok(obj.as_ptr());
    }
    Err(magnus::Error::new(
        ruby_helpers::type_error(),
        "Cannot convert Ruby value to Python",
    ))
}

/// A Ruby object that wraps a Python object.
/// Handles cross-language GC coordination.
#[magnus::wrap(class = "RubyxObject", mark, free_immediately, size)]
pub struct RubyxObject {
    py_obj: *mut PyObject,
    api: &'static PythonApi,
}
unsafe impl Send for RubyxObject {}
unsafe impl Sync for RubyxObject {}
impl RubyxObject {
    /// Create a new wrapper, incrementing the Python object's reference count.
    pub fn new(py_obj: *mut PyObject, api: &'static PythonApi) -> Option<Self> {
        if py_obj.is_null() {
            return None;
        }
        if !api.is_initialized() {
            return None;
        }
        // ensure_gil is reentrant — safe even if caller already holds GIL
        let gil = api.ensure_gil();
        // Increase refcount
        api.incref(py_obj);
        api.release_gil(gil);
        Some(RubyxObject { py_obj, api })
    }

    pub fn as_ptr(&self) -> *mut PyObject {
        self.py_obj
    }

    /// This method provides a dynamic dispatch mechanism to resolve and call methods on Python objects
    /// in a Ruby environment using the `magnus` bridge and internal Python C API bindings.
    ///
    /// The `method_missing` function is the Ruby equivalent of handling undefined method calls (e.g., `obj.foo`)
    /// on a Ruby object, but it utilizes Python interop to dynamically retrieve, set, or invoke Python attributes
    /// and methods, depending on the method call's context.
    ///
    /// # Arguments
    /// - `&self`: Reference to the current object which interacts with a Python object.
    /// - `args`: A slice of `magnus::Value` that represents Ruby arguments. This typically includes:
    ///   * The name of the method being called as a Symbol/String.
    ///   * Any additional arguments for a method call or value in the case of setters.
    /// # Returns
    /// - `Result<magnus::Value, magnus::Error>`:
    ///   * On success, returns a `magnus::Value` object that represents the result of the Python interaction,
    ///     whether it's an attribute access, setter operation, or method call.
    ///   * On failure, returns a `magnus::Error` containing details about the failure reason.
    ///
    /// # Error Handling
    /// - Raises `magnus::Error` for invalid invocation patterns:
    ///   * If `args` is empty.
    ///   * If the method name is not a valid String or Symbol.
    ///   * If the method attempts a setter operation with an incorrect number of arguments.
    /// - Handles Ruby and Python exceptions during API interop by translating them into appropriate `magnus::Error`s.
    /// # Examples
    /// ```ruby
    /// obj.foo         # Triggers a Python attribute getter
    /// obj.foo(1, 2)   # Triggers a Python method call with positional arguments
    /// obj.foo = value # Triggers a Python attribute setter
    /// ```
    ///
    /// ## Ruby Code to `args` Slice Mapping
    ///
    /// The `args` parameter is a flat slice where `args[0]` is always the method name
    /// (Symbol or String), and the remaining elements are the call arguments. Ruby's
    /// `method_missing(*args)` (declared with arity `-1` in Magnus) packs everything
    /// into this single slice.
    ///
    /// | Ruby Code                        | `args` Slice                                       | Dispatch Path     |
    /// |----------------------------------|-----------------------------------------------------|-------------------|
    /// | `obj.foo`                        | `[:foo]`                                            | Getter            |
    /// | `obj.foo = 42`                   | `[:"foo=", 42]`                                    | Setter            |
    /// | `obj.foo(1, 2)`                  | `[:foo, 1, 2]`                                     | Call (positional)  |
    /// | `obj.foo(a, k: v)`               | `[:foo, a, {k: v}]`                                | Call (pos + kwargs)|
    /// | `obj.dumps(data, indent: 2)`     | `[:dumps, data, {indent: 2}]`                      | Call (pos + kwargs)|
    ///
    /// ### Getter (`args.len() == 1`, no `=` suffix)
    /// ```ruby
    /// obj.foo         # args = [:foo]
    /// ```
    /// Resolves via `PyObject_GetAttrString`. If the attribute is non-callable, it is
    /// returned directly as a wrapped `RubyxObject`.
    ///
    /// ### Setter (`args[0]` ends with `=`, `args.len() == 2`)
    /// ```ruby
    /// obj.foo = value # args = [:"foo=", value]
    /// ```
    /// The trailing `=` is stripped to get the attribute name, then
    /// `PyObject_SetAttrString` is called with the converted Python value.
    ///
    /// ### Callable (`args.len() > 1`, or attribute is callable)
    /// ```ruby
    /// obj.foo(1, 2)              # args = [:foo, 1, 2]          → positional only
    /// obj.foo(1, key: "val")     # args = [:foo, 1, {key: "val"}] → positional + kwargs
    /// ```
    /// Positional arguments are `args[1..]` (excluding a trailing Hash). If the last
    /// element in `args[1..]` is a Ruby `Hash`, it is split off and converted to a
    /// Python kwargs dict. A Python tuple is built from the positional arguments, and
    /// the call is dispatched via `PyObject_Call(callable, args_tuple, kwargs_dict)`.
    ///
    /// # Limitations
    /// - Currently restricted to single inheritance where the missing Ruby method maps directly to a single Python
    ///   object interaction.
    /// - Keyword arguments (kwargs) are only supported if the last Ruby argument is a hash that can be converted to a Python dict.
    pub fn method_missing(&self, args: &[magnus::Value]) -> Result<magnus::Value, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();

        // Get python attribute if exist
        let result = (|| -> Result<Value, magnus::Error> {
            if args.is_empty() {
                return Err(magnus::Error::new(
                    ruby_helpers::arg_error(),
                    "No method name given",
                ));
            }
            let ruby = Ruby::get().map_err(|e| {
                magnus::Error::new(
                    ruby_helpers::runtime_error(),
                    format!("Ruby VM handle unavailable: {e}"),
                )
            })?;
            let method_name = if let Ok(s) = String::try_convert(args[0]) {
                s
            } else if let Ok(sym) = Symbol::try_convert(args[0]) {
                sym.name()?.to_string()
            } else {
                return Err(magnus::Error::new(
                    ruby_helpers::type_error(),
                    "method_missing expects Symbol/String method name",
                ));
            };

            if RUBY_IMPLICIT_CONVERSIONS.contains(&method_name.as_str()) {
                return Err(magnus::Error::new(
                    ruby_helpers::no_method_error(),
                    format!("undefined method '{}' for RubyxObject", method_name),
                ));
            }

            // Setter - `obj.foo = value`
            if method_name.ends_with("=") {
                if args.len() != 2 {
                    return Err(magnus::Error::new(
                        ruby_helpers::arg_error(),
                        "Setter required exactly one value",
                    ));
                }
                let attr_name = &method_name[..method_name.len() - 1];
                let py_value = ruby_to_python(args[1], api)?;
                let rc = api.object_set_attr_string(self.py_obj, attr_name, py_value);
                api.decref(py_value); // set_attr_string does not steal reference
                if rc != 0 {
                    if let Some(py_err) = PythonApi::extract_exception(api) {
                        return Err(magnus::Error::from(py_err));
                    }
                    return Err(magnus::Error::new(
                        ruby_helpers::runtime_error(),
                        "Failed to set Python attribute",
                    ));
                }
                return Ok(args[1]);
            }
            // Getter - `obj.foo`
            let python_attr = api.object_get_attr_string(self.py_obj, &method_name);
            if python_attr.is_null() {
                api.clear_error();
                return Err(magnus::Error::new(
                    ruby_helpers::exception(),
                    format!("undefined method `{method_name}` for a Python object"),
                ));
            }
            let py_attr_guard = PyGuard::new(python_attr, api).ok_or_else(|| {
                magnus::Error::new(ruby_helpers::runtime_error(), "Null Python attribute")
            })?;

            // Attribute read path (non-callable + no args) - `obj.foo`
            if api.callable_check(py_attr_guard.ptr()) == 0 && args.len() == 1 {
                let wrapper = RubyxObject::new(py_attr_guard.ptr(), api).ok_or_else(|| {
                    magnus::Error::new(
                        ruby_helpers::runtime_error(),
                        "Failed to wrap Python attribute",
                    )
                })?;
                return Ok(wrapper.into_value_with(&ruby));
            }
            // Call path - `obj.foo(args)`
            let call_args = &args[1..];

            // Optional kwargs: last arg hash
            let (positional, kwargs) = if let Some(last) = call_args.last() {
                if last.is_kind_of(ruby.class_hash()) {
                    (
                        &call_args[..call_args.len() - 1],
                        Some(RHash::try_convert(*last)?),
                    )
                } else {
                    (call_args, None)
                }
            } else {
                (call_args, None)
            };

            // Args Tuple for args
            let py_args = api.tuple_new(positional.len() as isize);
            if py_args.is_null() {
                return Err(magnus::Error::new(
                    ruby_helpers::runtime_error(),
                    "Failed to allocate Python args tuple",
                ));
            }
            let py_args_guard = PyGuard::new(py_args, api).ok_or_else(|| {
                magnus::Error::new(ruby_helpers::runtime_error(), "Null Python args tuple")
            })?;
            for (i, arg) in positional.iter().enumerate() {
                let py_arg = ruby_to_python(*arg, api)?;
                // tuple_set_item steals reference on success
                if api.tuple_set_item(py_args_guard.ptr(), i as isize, py_arg) != 0 {
                    api.decref(py_arg); // only decref on failure
                    if let Some(py_err) = PythonApi::extract_exception(api) {
                        return Err(magnus::Error::from(py_err));
                    }
                    return Err(magnus::Error::new(
                        ruby_helpers::runtime_error(),
                        "Failed to set tuple argument",
                    ));
                }
            }
            // Kwargs Dict for kwargs
            let py_kwargs_guard = if let Some(hash) = kwargs {
                // Convert kwargs to Python dict
                let dict = api.dict_new();
                if dict.is_null() {
                    return Err(magnus::Error::new(
                        ruby_helpers::runtime_error(),
                        "Failed to allocate kwargs dict",
                    ));
                }
                let guard = PyGuard::new(dict, api).ok_or_else(|| {
                    magnus::Error::new(ruby_helpers::runtime_error(), "Null kwargs dict")
                })?;
                // Save the key and value to python dict
                hash.foreach(|k: Value, v: Value| {
                    let key = if let Ok(s) = String::try_convert(k) {
                        s
                    } else if let Ok(sym) = Symbol::try_convert(k) {
                        sym.name()?.to_string()
                    } else {
                        return Err(magnus::Error::new(
                            ruby_helpers::type_error(),
                            "kwargs keys must be String or Symbol",
                        ));
                    };
                    let py_key = key.to_python(api).map_err(|e| {
                        magnus::Error::new(ruby_helpers::runtime_error(), format!("{e:?}"))
                    })?;
                    let py_val = ruby_to_python(v, api)?;
                    let rc = api.dict_set_item(guard.ptr(), py_key, py_val);
                    // dict_set_item does not steal
                    api.decref(py_key);
                    api.decref(py_val);
                    if rc != 0 {
                        if let Some(py_err) = PythonApi::extract_exception(api) {
                            return Err(magnus::Error::from(py_err));
                        }
                        return Err(magnus::Error::new(
                            ruby_helpers::runtime_error(),
                            "Failed to set kwargs item",
                        ));
                    }
                    Ok(ForEach::Continue)
                })?;
                Some(guard)
            } else {
                None
            };
            let py_kwargs_ptr = py_kwargs_guard
                .as_ref()
                .map_or(std::ptr::null_mut(), |g| g.ptr());
            let py_result =
                api.object_call(py_attr_guard.ptr(), py_args_guard.ptr(), py_kwargs_ptr);
            if py_result.is_null() {
                if let Some(py_err) = PythonApi::extract_exception(api) {
                    return Err(magnus::Error::from(py_err));
                }
                return Err(magnus::Error::new(
                    ruby_helpers::runtime_error(),
                    "Python call failed",
                ));
            }
            let py_result_guard = PyGuard::new(py_result, api).ok_or_else(|| {
                magnus::Error::new(ruby_helpers::runtime_error(), "Null Python result")
            })?;
            let wrapper = RubyxObject::new(py_result_guard.ptr(), api).ok_or_else(|| {
                magnus::Error::new(
                    ruby_helpers::runtime_error(),
                    "Failed to wrap a Python result",
                )
            })?;
            Ok(wrapper.into_value_with(&ruby))
        })();
        api.release_gil(gil);
        result
    }

    pub fn respond_to_missing(&self, args: &[magnus::Value]) -> Result<bool, magnus::Error> {
        if args.is_empty() {
            return Err(magnus::Error::new(
                ruby_helpers::arg_error(),
                "No method name given",
            ));
        }
        let name = if let Ok(s) = String::try_convert(args[0]) {
            s
        } else if let Ok(sym) = Symbol::try_convert(args[0]) {
            sym.name()?.to_string()
        } else {
            return Err(magnus::Error::new(
                ruby_helpers::type_error(),
                "method_missing expects Symbol/String method name",
            ));
        };

        let api = crate::api();
        let gil = api.ensure_gil();
        let c_name = CString::new(name.as_str())
            .map_err(|_| magnus::Error::new(ruby_helpers::arg_error(), "Invalid method name"))?;
        let result = api.object_has_attr_string(self.as_ptr(), c_name.as_ptr()) != 0;
        api.release_gil(gil);
        Ok(result)
    }

    pub fn to_s(&self) -> Result<String, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();
        let py_str = api.object_str(self.as_ptr());
        let result = if py_str.is_null() {
            api.clear_error();
            format!("#<RubyxObject:{:p}>", self.as_ptr())
        } else {
            let s = api.string_to_string(py_str).unwrap_or_default();
            api.decref(py_str);
            s
        };

        api.release_gil(gil);
        Ok(result)
    }

    pub fn inspect(&self) -> Result<String, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();
        let result = api.object_repr(self.as_ptr());

        api.release_gil(gil);
        Ok(result)
    }

    pub fn to_ruby(&self) -> Result<magnus::Value, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();

        let sendable = python_to_sendable(self.as_ptr(), api)
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e));

        api.release_gil(gil);

        sendable?.try_into()
    }

    pub fn getitem(&self, key: Value) -> Result<Value, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();
        let ruby = Ruby::get()
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e.to_string()))?;

        let py_key = ruby_to_python(key, api)?;
        let result = api.object_get_item(self.as_ptr(), py_key);
        api.decref(py_key);

        if result.is_null() {
            let err = if let Some(exc) = PythonApi::extract_exception(api) {
                magnus::Error::from(exc)
            } else {
                magnus::Error::new(ruby_helpers::runtime_error(), "KeyError or IndexError")
            };
            api.release_gil(gil);
            return Err(err);
        }

        let wrapper = RubyxObject::new(result, api).ok_or_else(|| {
            magnus::Error::new(ruby_helpers::runtime_error(), "Failed to wrap result")
        })?;
        api.release_gil(gil);
        Ok(wrapper.into_value_with(&ruby))
    }

    pub fn setitem(&self, key: Value, value: Value) -> Result<Value, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();

        let py_key = ruby_to_python(key, api)?;
        let py_val = ruby_to_python(value, api)?;
        let result = api.object_set_item(self.as_ptr(), py_key, py_val);
        api.decref(py_key);
        api.decref(py_val);

        if result == -1 {
            let err = if let Some(exc) = PythonApi::extract_exception(api) {
                magnus::Error::from(exc)
            } else {
                magnus::Error::new(ruby_helpers::runtime_error(), "Failed to set item")
            };
            api.release_gil(gil);
            return Err(err);
        }

        api.release_gil(gil);
        Ok(value)
    }

    pub fn delitem(&self, key: Value) -> Result<Value, magnus::Error> {
        let api = crate::api();
        let gil = api.ensure_gil();
        let ruby = Ruby::get()
            .map_err(|e| magnus::Error::new(ruby_helpers::runtime_error(), e.to_string()))?;

        let py_key = ruby_to_python(key, api)?;
        let result = api.object_del_item(self.as_ptr(), py_key);
        api.decref(py_key);

        if result == -1 {
            let err = if let Some(exc) = PythonApi::extract_exception(api) {
                magnus::Error::from(exc)
            } else {
                magnus::Error::new(ruby_helpers::runtime_error(), "Failed to delete item")
            };
            api.release_gil(gil);
            return Err(err);
        }

        api.release_gil(gil);
        Ok(ruby.qnil().as_value())
    }
}

impl Drop for RubyxObject {
    fn drop(&mut self) {
        // Python object no longer exist
        if self.py_obj.is_null() {
            return;
        }
        // Python api does not exist
        if !self.api.is_initialized() {
            return;
        }
        // Lock gil
        let gil = self.api.ensure_gil();
        self.api.decref(self.py_obj);
        self.api.release_gil(gil);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_ruby_python;
    use magnus::{IntoValue, TryConvert};
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_ruby_to_python_primitives() {
        with_ruby_python(|ruby, api| {
            let py_nil =
                ruby_to_python(ruby.qnil().as_value(), api).expect("nil conversion should succeed");
            assert!(api.is_none(py_nil));
            api.decref(py_nil);

            let py_true = ruby_to_python(true.into_value_with(ruby), api)
                .expect("true conversion should succeed");
            assert!(api.is_true(py_true));
            api.decref(py_true);

            let py_int = ruby_to_python(42_i64.into_value_with(ruby), api)
                .expect("int conversion should succeed");
            assert_eq!(api.long_to_i64(py_int), 42);
            api.decref(py_int);

            let py_float = ruby_to_python(3.5_f64.into_value_with(ruby), api)
                .expect("float conversion should succeed");
            assert!(api.is_float(py_float));
            assert!((api.float_to_f64(py_float) - 3.5).abs() < 1e-9);
            api.decref(py_float);

            let py_str = ruby_to_python("hello".into_value_with(ruby), api)
                .expect("string conversion should succeed");
            assert_eq!(api.string_to_string(py_str), Some("hello".to_string()));
            api.decref(py_str);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_calls_python_callable() {
        with_ruby_python(|ruby, api| {
            let json = api.import_module("json").expect("json module must import");
            let wrapper = RubyxObject::new(json, api).expect("wrapper should be created");

            let args = vec![
                "loads".into_value_with(ruby),
                "[1, 2, 3]".into_value_with(ruby),
            ];
            let result = wrapper
                .method_missing(&args)
                .expect("loads call should succeed");
            let py_result = Obj::<RubyxObject>::try_convert(result)
                .expect("result should be wrapped Python object");
            assert!(api.list_check(py_result.as_ptr()));
            assert_eq!(api.list_size(py_result.as_ptr()), 3);

            drop(wrapper);
            api.decref(json);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_reads_non_callable_attribute() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys module must import");
            let wrapper = RubyxObject::new(sys, api).expect("wrapper should be created");

            let args = vec!["version".into_value_with(ruby)];
            let result = wrapper
                .method_missing(&args)
                .expect("attribute read should succeed");
            let py_result = Obj::<RubyxObject>::try_convert(result)
                .expect("result should be wrapped Python object");
            assert!(api.is_string(py_result.as_ptr()));
            let version = api
                .string_to_string(py_result.as_ptr())
                .expect("version should decode as string");
            assert!(!version.is_empty());
            println!("Python version: {}", version);

            drop(wrapper);
            api.decref(sys);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_returns_error_for_unknown_member() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys module must import");
            let wrapper = RubyxObject::new(sys, api).expect("wrapper should be created");

            let args = vec!["this_member_should_not_exist_abc123".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_err());

            drop(wrapper);
            api.decref(sys);
        });
    }

    // ========== to_s tests ==========

    #[test]
    #[serial]
    fn test_to_s_returns_python_str_for_int() {
        use crate::test_helpers::skip_if_no_python;
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let py_int = api.long_from_i64(99);
        let wrapper = RubyxObject::new(py_int, api).unwrap();
        assert_eq!(wrapper.to_s().unwrap(), "99");
        drop(wrapper);
        api.decref(py_int);
    }

    #[test]
    #[serial]
    fn test_to_s_returns_python_str_for_string() {
        use crate::test_helpers::skip_if_no_python;
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let py_str = api.string_from_str("world");
        let wrapper = RubyxObject::new(py_str, api).unwrap();
        assert_eq!(wrapper.to_s().unwrap(), "world");
        drop(wrapper);
        api.decref(py_str);
    }

    #[test]
    #[serial]
    fn test_to_s_returns_python_str_for_none() {
        use crate::test_helpers::skip_if_no_python;
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        api.incref(api.py_none);
        let wrapper = RubyxObject::new(api.py_none, api).unwrap();
        assert_eq!(wrapper.to_s().unwrap(), "None");
        drop(wrapper);
    }

    // ========== inspect tests ==========

    #[test]
    #[serial]
    fn test_inspect_returns_repr_for_int() {
        use crate::test_helpers::skip_if_no_python;
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let py_int = api.long_from_i64(7);
        let wrapper = RubyxObject::new(py_int, api).unwrap();
        assert_eq!(wrapper.inspect().unwrap(), "7");
        drop(wrapper);
        api.decref(py_int);
    }

    #[test]
    #[serial]
    fn test_inspect_returns_repr_for_string_with_quotes() {
        use crate::test_helpers::skip_if_no_python;
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let py_str = api.string_from_str("test");
        let wrapper = RubyxObject::new(py_str, api).unwrap();
        // Python repr of string includes quotes
        assert_eq!(wrapper.inspect().unwrap(), "'test'");
        drop(wrapper);
        api.decref(py_str);
    }

    // ========== to_ruby tests ==========

    #[test]
    #[serial]
    fn test_to_ruby_converts_int() {
        with_ruby_python(|_ruby, api| {
            let py_int = api.long_from_i64(123);
            let wrapper = RubyxObject::new(py_int, api).unwrap();
            let ruby_val = wrapper.to_ruby().expect("to_ruby should succeed");
            assert_eq!(i64::try_convert(ruby_val).unwrap(), 123);
            drop(wrapper);
            api.decref(py_int);
        });
    }

    #[test]
    #[serial]
    fn test_to_ruby_converts_string() {
        with_ruby_python(|_ruby, api| {
            let py_str = api.string_from_str("rubyx");
            let wrapper = RubyxObject::new(py_str, api).unwrap();
            let ruby_val = wrapper.to_ruby().expect("to_ruby should succeed");
            assert_eq!(String::try_convert(ruby_val).unwrap(), "rubyx");
            drop(wrapper);
            api.decref(py_str);
        });
    }

    #[test]
    #[serial]
    fn test_to_ruby_converts_float() {
        with_ruby_python(|_ruby, api| {
            let py_float = api.float_from_f64(2.718);
            let wrapper = RubyxObject::new(py_float, api).unwrap();
            let ruby_val = wrapper.to_ruby().expect("to_ruby should succeed");
            let f = f64::try_convert(ruby_val).unwrap();
            assert!((f - 2.718).abs() < 0.001);
            drop(wrapper);
            api.decref(py_float);
        });
    }

    #[test]
    #[serial]
    fn test_to_ruby_converts_bool() {
        with_ruby_python(|_ruby, api| {
            let py_true = api.bool_from_i64(1);
            let wrapper = RubyxObject::new(py_true, api).unwrap();
            let ruby_val = wrapper.to_ruby().expect("to_ruby should succeed");
            assert!(bool::try_convert(ruby_val).unwrap());
            drop(wrapper);
            api.decref(py_true);
        });
    }

    #[test]
    #[serial]
    fn test_to_ruby_converts_none_to_nil() {
        with_ruby_python(|_ruby, api| {
            api.incref(api.py_none);
            let wrapper = RubyxObject::new(api.py_none, api).unwrap();
            let ruby_val = wrapper.to_ruby().expect("to_ruby should succeed");
            assert!(magnus::value::ReprValue::is_nil(ruby_val));
            drop(wrapper);
        });
    }

    #[test]
    #[serial]
    fn test_to_ruby_errors_for_module() {
        with_ruby_python(|_ruby, api| {
            let module = api.import_module("os").expect("os should import");
            let wrapper = RubyxObject::new(module, api).unwrap();
            assert!(
                wrapper.to_ruby().is_err(),
                "module should not convert to Ruby"
            );
            drop(wrapper);
            api.decref(module);
        });
    }

    // ========== method_missing with args ==========

    #[test]
    #[serial]
    fn test_method_missing_with_no_args_returns_error() {
        with_ruby_python(|_ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            let result = wrapper.method_missing(&[]);
            assert!(result.is_err(), "empty args should error");

            drop(wrapper);
            api.decref(sys);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_chained_calls() {
        with_ruby_python(|ruby, api| {
            let json = api.import_module("json").expect("json should import");
            let wrapper = RubyxObject::new(json, api).unwrap();

            // json.dumps(json.loads("[1,2]"))
            let loads_args = vec![
                "loads".into_value_with(ruby),
                "[1, 2, 3]".into_value_with(ruby),
            ];
            let list_result = wrapper
                .method_missing(&loads_args)
                .expect("loads should succeed");

            let list_wrapper =
                Obj::<RubyxObject>::try_convert(list_result).expect("should be RubyxObject");
            assert!(api.list_check(list_wrapper.as_ptr()));

            drop(wrapper);
            api.decref(json);
        });
    }

    // ========== respond_to_missing? tests ==========

    #[test]
    #[serial]
    fn test_respond_to_missing_existing_attr() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            // sys.version exists
            let args = vec!["version".into_value_with(ruby)];
            let result = wrapper.respond_to_missing(&args).expect("should not error");
            assert!(result, "sys.version should exist");

            drop(wrapper);
            api.decref(sys);
        });
    }

    #[test]
    #[serial]
    fn test_respond_to_missing_nonexistent_attr() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            let args = vec!["nonexistent_xyz_123".into_value_with(ruby)];
            let result = wrapper.respond_to_missing(&args).expect("should not error");
            assert!(!result, "nonexistent attr should return false");

            drop(wrapper);
            api.decref(sys);
        });
    }

    #[test]
    #[serial]
    fn test_respond_to_missing_callable_method() {
        with_ruby_python(|ruby, api| {
            let json = api.import_module("json").expect("json should import");
            let wrapper = RubyxObject::new(json, api).unwrap();

            let args = vec!["loads".into_value_with(ruby)];
            let result = wrapper.respond_to_missing(&args).expect("should not error");
            assert!(result, "json.loads should exist");

            drop(wrapper);
            api.decref(json);
        });
    }

    #[test]
    #[serial]
    fn test_respond_to_missing_with_string_arg() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            // Pass string instead of symbol
            let args = vec!["version".into_value_with(ruby)];
            let result = wrapper.respond_to_missing(&args).expect("should not error");
            assert!(result, "should accept string arg too");

            drop(wrapper);
            api.decref(sys);
        });
    }

    #[test]
    #[serial]
    fn test_respond_to_missing_empty_args_errors() {
        with_ruby_python(|_ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            let result = wrapper.respond_to_missing(&[]);
            assert!(result.is_err(), "empty args should error");

            drop(wrapper);
            api.decref(sys);
        });
    }

    // ========== implicit conversion guards ==========

    #[test]
    #[serial]
    fn test_method_missing_guards_to_ary() {
        with_ruby_python(|ruby, api| {
            let py_int = api.long_from_i64(42);
            let wrapper = RubyxObject::new(py_int, api).unwrap();

            let args = vec!["to_ary".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_err(), "to_ary should be guarded");

            drop(wrapper);
            api.decref(py_int);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_guards_to_str() {
        with_ruby_python(|ruby, api| {
            let py_int = api.long_from_i64(42);
            let wrapper = RubyxObject::new(py_int, api).unwrap();

            let args = vec!["to_str".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_err(), "to_str should be guarded");

            drop(wrapper);
            api.decref(py_int);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_guards_to_hash() {
        with_ruby_python(|ruby, api| {
            let py_int = api.long_from_i64(42);
            let wrapper = RubyxObject::new(py_int, api).unwrap();

            let args = vec!["to_hash".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_err(), "to_hash should be guarded");

            drop(wrapper);
            api.decref(py_int);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_guards_to_int() {
        with_ruby_python(|ruby, api| {
            let py_int = api.long_from_i64(42);
            let wrapper = RubyxObject::new(py_int, api).unwrap();

            let args = vec!["to_int".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_err(), "to_int should be guarded");

            drop(wrapper);
            api.decref(py_int);
        });
    }

    #[test]
    #[serial]
    fn test_method_missing_allows_regular_methods() {
        with_ruby_python(|ruby, api| {
            let sys = api.import_module("sys").expect("sys should import");
            let wrapper = RubyxObject::new(sys, api).unwrap();

            // "version" is not guarded — should delegate to Python
            let args = vec!["version".into_value_with(ruby)];
            let result = wrapper.method_missing(&args);
            assert!(result.is_ok(), "regular attributes should not be guarded");

            drop(wrapper);
            api.decref(sys);
        });
    }

    // ========== getitem / setitem / delitem tests ==========

    #[test]
    #[serial]
    fn test_getitem_dict_string_key() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{'name': 'Alice', 'age': 30}", 258, globals.ptr(), globals.ptr())
                .expect("should create dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "name".into_value_with(ruby);
            let result = wrapper.getitem(key).expect("getitem should succeed");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(
                api.string_to_string(obj.as_ptr()),
                Some("Alice".to_string())
            );

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_getitem_dict_integer_key() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{1: 'one', 2: 'two'}", 258, globals.ptr(), globals.ptr())
                .expect("should create dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = 1_i64.into_value_with(ruby);
            let result = wrapper.getitem(key).expect("getitem should succeed");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(
                api.string_to_string(obj.as_ptr()),
                Some("one".to_string())
            );

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_getitem_list_by_index() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_list = api
                .run_string("[10, 20, 30]", 258, globals.ptr(), globals.ptr())
                .expect("should create list");
            let wrapper = RubyxObject::new(py_list, api).unwrap();

            let key: magnus::Value = 1_i64.into_value_with(ruby);
            let result = wrapper.getitem(key).expect("getitem should succeed");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 20);

            drop(wrapper);
            api.decref(py_list);
        });
    }

    #[test]
    #[serial]
    fn test_getitem_list_negative_index() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_list = api
                .run_string("[10, 20, 30]", 258, globals.ptr(), globals.ptr())
                .expect("should create list");
            let wrapper = RubyxObject::new(py_list, api).unwrap();

            // Python supports negative indexing
            let key: magnus::Value = (-1_i64).into_value_with(ruby);
            let result = wrapper.getitem(key).expect("getitem should succeed");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 30);

            drop(wrapper);
            api.decref(py_list);
        });
    }

    #[test]
    #[serial]
    fn test_getitem_missing_key_raises_error() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{}", 258, globals.ptr(), globals.ptr())
                .expect("should create empty dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "nope".into_value_with(ruby);
            let result = wrapper.getitem(key);
            assert!(result.is_err(), "missing key should raise error");

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_getitem_index_out_of_range() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_list = api
                .run_string("[1, 2]", 258, globals.ptr(), globals.ptr())
                .expect("should create list");
            let wrapper = RubyxObject::new(py_list, api).unwrap();

            let key: magnus::Value = 99_i64.into_value_with(ruby);
            let result = wrapper.getitem(key);
            assert!(result.is_err(), "out of range index should raise error");

            drop(wrapper);
            api.decref(py_list);
        });
    }

    #[test]
    #[serial]
    fn test_setitem_dict() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{}", 258, globals.ptr(), globals.ptr())
                .expect("should create empty dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "role".into_value_with(ruby);
            let val: magnus::Value = "admin".into_value_with(ruby);
            wrapper.setitem(key, val).expect("setitem should succeed");

            // Verify the value was set
            let check_key: magnus::Value = "role".into_value_with(ruby);
            let result = wrapper.getitem(check_key).expect("should find new key");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(
                api.string_to_string(obj.as_ptr()),
                Some("admin".to_string())
            );

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_setitem_list() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_list = api
                .run_string("[1, 2, 3]", 258, globals.ptr(), globals.ptr())
                .expect("should create list");
            let wrapper = RubyxObject::new(py_list, api).unwrap();

            let key: magnus::Value = 1_i64.into_value_with(ruby);
            let val: magnus::Value = 99_i64.into_value_with(ruby);
            wrapper.setitem(key, val).expect("setitem should succeed");

            // Verify
            let check_key: magnus::Value = 1_i64.into_value_with(ruby);
            let result = wrapper.getitem(check_key).expect("should read index 1");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 99);

            drop(wrapper);
            api.decref(py_list);
        });
    }

    #[test]
    #[serial]
    fn test_setitem_overwrite_existing() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{'x': 1}", 258, globals.ptr(), globals.ptr())
                .expect("should create dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "x".into_value_with(ruby);
            let val: magnus::Value = 42_i64.into_value_with(ruby);
            wrapper.setitem(key, val).expect("setitem should succeed");

            let check_key: magnus::Value = "x".into_value_with(ruby);
            let result = wrapper.getitem(check_key).expect("should read key");
            let obj = Obj::<RubyxObject>::try_convert(result).expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 42);

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_delitem_dict() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{'a': 1, 'b': 2}", 258, globals.ptr(), globals.ptr())
                .expect("should create dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "a".into_value_with(ruby);
            wrapper.delitem(key).expect("delitem should succeed");

            // Verify 'a' is gone
            let check_key: magnus::Value = "a".into_value_with(ruby);
            let result = wrapper.getitem(check_key);
            assert!(result.is_err(), "'a' should be deleted");

            // Verify 'b' still exists
            let check_key_b: magnus::Value = "b".into_value_with(ruby);
            let result_b = wrapper.getitem(check_key_b).expect("'b' should still exist");
            let obj = Obj::<RubyxObject>::try_convert(result_b).expect("should be RubyxObject");
            assert_eq!(api.long_to_i64(obj.as_ptr()), 2);

            drop(wrapper);
            api.decref(py_dict);
        });
    }

    #[test]
    #[serial]
    fn test_delitem_missing_key_raises_error() {
        with_ruby_python(|ruby, api| {
            let globals = crate::eval::make_globals(api);
            let py_dict = api
                .run_string("{}", 258, globals.ptr(), globals.ptr())
                .expect("should create empty dict");
            let wrapper = RubyxObject::new(py_dict, api).unwrap();

            let key: magnus::Value = "nope".into_value_with(ruby);
            let result = wrapper.delitem(key);
            assert!(result.is_err(), "deleting missing key should error");

            drop(wrapper);
            api.decref(py_dict);
        });
    }
}
