use crate::python_api::PythonApi;
use crate::python_guard::PyGuard;
use crate::ruby_helpers::runtime_error;
use crate::rubyx_object::RubyxObject;
use magnus::{Error, IntoValue, Ruby, Value};

pub(crate) fn rubyx_import(module_name: String) -> Result<Value, Error> {
    let api = crate::api();

    // Lock python gil
    let gil = api.ensure_gil();

    let result = (|| -> Result<Value, Error> {
        let ruby = Ruby::get().map_err(|e| {
            Error::new(runtime_error(), format!("Ruby VM unavailable: {e}"))
        })?;
        let module = match api.import_module(module_name.as_str()) {
            Ok(module) => module,
            Err(msg) => {
                if let Some(err) = PythonApi::extract_exception(api) {
                    return Err(Error::from(err));
                }
                return Err(Error::new(runtime_error(), msg));
            }
        };
        let py_module_guard = PyGuard::new(module, api)
            .ok_or_else(|| Error::new(runtime_error(), "Python returned null result"))?;
        let wrapper = RubyxObject::new(py_module_guard.ptr(), api)
            .ok_or_else(|| Error::new(runtime_error(), "Failed to create RubyxObject"))?;
        Ok(wrapper.into_value_with(&ruby))
    })();

    api.release_gil(gil);
    result
}
