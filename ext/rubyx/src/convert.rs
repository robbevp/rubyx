
use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum ConvertError {
    #[error("Type error: expected {expected}, got {got}")]
    TypeError { expected: &'static str, got: String },

    #[error("Integer overflow")]
    Overflow,

    #[error("Python error: {0}")]
    PythonError(String),

    #[error("Encoding error")]
    EncodingError,
}

// TODO: Implement ToPython trait
// pub trait ToPython {
//     fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError>;
// }
#[allow(dead_code)]
pub trait ToPython {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError>;
}

#[allow(dead_code)]
pub trait FromPython: Sized {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError>;
}

// TODO: Implement ToPython for i64
impl ToPython for i64 {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let obj = api.long_from_i64(*self);
        if obj.is_null() {
            // Check if Python failed to create object
            Err(ConvertError::PythonError("Failed to create int".into()))
        } else {
            Ok(obj)
        }
    }
}
// TODO: Implement FromPython for i64
impl FromPython for i64 {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        // Type check
        if !api.is_long(obj) {
            return Err(ConvertError::TypeError {
                expected: "int",
                got: "other".to_string(),
            });
        }

        // Extract value after type check passes
        let value = api.long_to_i64(obj);

        // Check for overflow
        if value == -1 && api.has_error() {
            // Need to clear the error before checking for overflow again
            api.clear_error();
            return Err(ConvertError::Overflow);
        }

        Ok(value)
    }
}
// TODO: Implement ToPython for f64
impl ToPython for f64 {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let obj = api.float_from_f64(*self);
        if obj.is_null() {
            return Err(ConvertError::PythonError("Failed to create float".into()));
        }
        Ok(obj)
    }
}

// TODO: Implement FromPython for f64
impl FromPython for f64 {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        if !api.is_float(obj) {
            return Err(ConvertError::TypeError {
                expected: "float",
                got: "other".to_string(),
            });
        }
        let value = api.float_to_f64(obj);
        if value == -1.0 && api.has_error() {
            api.clear_error();
            return Err(ConvertError::PythonError(
                "Failed to convert from Python float".into(),
            ));
        }
        Ok(value)
    }
}

// TODO: Implement ToPython for bool
impl ToPython for bool {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let obj = api.bool_from_bool(*self);
        api.incref(obj);
        Ok(obj)
    }
}

// TODO: Implement FromPython for bool
impl FromPython for bool {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        api.bool_to_bool(obj).map_err(|_| ConvertError::TypeError {
            expected: "bool",
            got: "other".to_string(),
        })
    }
}

impl ToPython for &str {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let obj = api.string_from_str(self);
        if obj.is_null() {
            Err(ConvertError::PythonError("Failed to create str".into()))
        } else {
            Ok(obj)
        }
    }
}

impl ToPython for String {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        self.as_str().to_python(api)
    }
}

impl FromPython for String {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        if !api.is_string(obj) {
            return Err(ConvertError::TypeError {
                expected: "str",
                got: "other".to_string(),
            });
        }
        api.string_to_string(obj).ok_or(ConvertError::EncodingError)
    }
}

// TODO: Implement ToPython for Option<T> where T: ToPython
impl<T: ToPython> ToPython for Option<T> {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        match self {
            Some(value) => value.to_python(api),
            None => {
                api.incref(api.py_none);
                Ok(api.py_none)
            }
        }
    }
}

// TODO: Implement ToPython for Vec<T>
impl<T: ToPython> ToPython for Vec<T> {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let py_list = api.list_new(self.len() as isize);
        if py_list.is_null() {
            return Err(ConvertError::PythonError("Failed to create list".into()));
        }
        for (index, item) in self.iter().enumerate() {
            let py_item = item.to_python(api)?;
            let result = api.list_set_item(py_list, index as isize, py_item);
            if result != 0 {
                // PyList_SetItem failed - it did NOT steal the reference
                api.decref(py_item);
                api.decref(py_list);
                return Err(ConvertError::PythonError("Failed to set list item".into()));
            }
            // Success: reference was stolen, don't decref py_item
        }
        Ok(py_list)
    }
}
// TODO: Implement FromPython for Vec<T>
impl<T: FromPython> FromPython for Vec<T> {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        if !api.list_check(obj) {
            return Err(ConvertError::TypeError {
                expected: "list",
                got: "other".to_string(),
            });
        }
        let size = api.list_size(obj) as usize;
        let mut list = Vec::with_capacity(size);

        for index in 0..size {
            let py_item = api.list_get_item(obj, index as isize);
            if py_item.is_null() {
                return Err(ConvertError::PythonError("Failed to get list item".into()));
            }
            let item = T::from_python(py_item, api)?;
            list.push(item);
        }
        Ok(list)
    }
}
// TODO: Implement ToPython for HashMap<K, V>
impl<K: ToPython, V: ToPython> ToPython for HashMap<K, V> {
    fn to_python(&self, api: &PythonApi) -> Result<*mut PyObject, ConvertError> {
        let dict = api.dict_new();
        if dict.is_null() {
            return Err(ConvertError::PythonError(
                "Failed to create dictionary".into(),
            ));
        }
        for (key, value) in self.iter() {
            let py_key = match key.to_python(api) {
                Ok(k) => k,
                Err(e) => {
                    api.decref(dict);
                    return Err(e);
                }
            };
            let py_value = match value.to_python(api) {
                Ok(v) => v,
                Err(e) => {
                    api.decref(dict);
                    api.decref(py_key);
                    return Err(e);
                }
            };
            let result = api.dict_set_item(dict, py_key, py_value);
            // PyDict_SetItem did NOT steal the reference
            api.decref(py_key);
            api.decref(py_value);
            if result == -1 {
                api.decref(dict);
                return Err(ConvertError::PythonError(
                    "Failed to set dictionary item".into(),
                ));
            }
        }
        Ok(dict)
    }
}
// TODO: Implement FromPython for HashMap<K, V>
impl<K: FromPython + Eq + std::hash::Hash, V: FromPython> FromPython for HashMap<K, V> {
    fn from_python(obj: *mut PyObject, api: &PythonApi) -> Result<Self, ConvertError> {
        if obj.is_null() {
            return Err(ConvertError::PythonError("Null object".into()));
        }
        if !api.dict_check(obj) {
            return Err(ConvertError::TypeError {
                expected: "dict",
                got: "other".to_string(),
            });
        }
        let mut map = HashMap::new();
        let mut position = 0;
        let mut key: *mut PyObject = std::ptr::null_mut();
        let mut value: *mut PyObject = std::ptr::null_mut();
        while api.dict_next(obj, &mut position, &mut key, &mut value) {
            let key = K::from_python(key, api)?;
            let value = V::from_python(value, api)?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::skip_if_no_python;
    use serial_test::serial;

    mod i64_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_i64_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = 42i64.to_python(api).unwrap();
            assert!(!py_obj.is_null());

            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 42);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_negative() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = (-123i64).to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, -123);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_max() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = i64::MAX.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, i64::MAX);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_min() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = i64::MIN.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, i64::MIN);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_zero() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = 0i64.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 0);

            api.decref(py_obj);
        }
    }

    mod f64_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_f64_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = std::f64::consts::PI.to_python(api).unwrap();
            assert!(!py_obj.is_null());

            let back = f64::from_python(py_obj, api).unwrap();
            assert!((back - std::f64::consts::PI).abs() < 1e-10);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_negative() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = (-2.5f64).to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert!((back - (-2.5)).abs() < 1e-10);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_zero() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = 0.0f64.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 0.0);

            api.decref(py_obj);
        }
    }

    mod bool_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_bool_true_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = true.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_true);
        }

        #[test]
        #[serial]
        fn test_bool_false_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = false.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_false);
        }

        #[test]
        #[serial]
        fn test_bool_roundtrip() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_true = true.to_python(api).unwrap();
            let back_true = bool::from_python(py_true, api).unwrap();
            assert!(back_true);

            let py_false = false.to_python(api).unwrap();
            let back_false = bool::from_python(py_false, api).unwrap();
            assert!(!back_false);
        }
    }

    mod string_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_string_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "hello world".to_python(api).unwrap();
            assert!(!py_obj.is_null());

            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "hello world");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_empty_string() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_unicode_string() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "hello".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "hello");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_with_spaces() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "  spaces  ".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "  spaces  ");

            api.decref(py_obj);
        }
    }

    mod option_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_none_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Option::<i64>::None.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_none);
        }

        #[test]
        #[serial]
        fn test_some_to_python() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some(42i64).to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);

            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 42);

            api.decref(py_obj);
        }
    }

    mod type_error_tests {
        use super::*;

        #[test]
        #[serial]
        fn test_string_from_int_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_int = 42i64.to_python(api).unwrap();
            let result = String::from_python(py_int, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "str");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_int);
        }

        #[test]
        #[serial]
        fn test_int_from_string_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_str = "not a number".to_python(api).unwrap();
            let result = i64::from_python(py_str, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "int");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_str);
        }

        #[test]
        #[serial]
        fn test_int_from_float_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_float = std::f64::consts::PI.to_python(api).unwrap();
            let result = i64::from_python(py_float, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "int");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_float);
        }

        #[test]
        #[serial]
        fn test_int_from_bool_succeeds() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_true = true.to_python(api).unwrap();
            assert_eq!(i64::from_python(py_true, api).unwrap(), 1);

            let py_false = false.to_python(api).unwrap();
            assert_eq!(i64::from_python(py_false, api).unwrap(), 0);
        }

        #[test]
        #[serial]
        fn test_float_from_int_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_int = 42i64.to_python(api).unwrap();
            let result = f64::from_python(py_int, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "float");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_int);
        }

        #[test]
        #[serial]
        fn test_float_from_string_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_str = "3.14".to_python(api).unwrap();
            let result = f64::from_python(py_str, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "float");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_str);
        }

        #[test]
        #[serial]
        fn test_float_from_bool_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_bool = true.to_python(api).unwrap();
            let result = f64::from_python(py_bool, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "float");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }

        #[test]
        #[serial]
        fn test_bool_from_int_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_int = 1i64.to_python(api).unwrap();
            let result = bool::from_python(py_int, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "bool");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_int);
        }

        #[test]
        #[serial]
        fn test_bool_from_string_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_str = "true".to_python(api).unwrap();
            let result = bool::from_python(py_str, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "bool");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_str);
        }

        #[test]
        #[serial]
        fn test_string_from_float_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_float = std::f64::consts::PI.to_python(api).unwrap();
            let result = String::from_python(py_float, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "str");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }

            api.decref(py_float);
        }

        #[test]
        #[serial]
        fn test_string_from_bool_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_bool = true.to_python(api).unwrap();
            let result = String::from_python(py_bool, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "str");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }

        #[test]
        #[serial]
        fn test_int_from_none_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let result = i64::from_python(api.py_none, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "int");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }

        #[test]
        #[serial]
        fn test_float_from_none_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let result = f64::from_python(api.py_none, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "float");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }

        #[test]
        #[serial]
        fn test_bool_from_none_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let result = bool::from_python(api.py_none, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "bool");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }

        #[test]
        #[serial]
        fn test_string_from_none_fails() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let result = String::from_python(api.py_none, api);

            assert!(result.is_err());
            match result.unwrap_err() {
                ConvertError::TypeError { expected, .. } => {
                    assert_eq!(expected, "str");
                }
                e => panic!("Expected TypeError, got {:?}", e),
            }
        }
    }

    mod f64_edge_cases {
        use super::*;

        #[test]
        #[serial]
        fn test_f64_max() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::MAX.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, f64::MAX);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_min() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::MIN.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, f64::MIN);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_min_positive() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::MIN_POSITIVE.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, f64::MIN_POSITIVE);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_infinity() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::INFINITY.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert!(back.is_infinite() && back.is_sign_positive());

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_neg_infinity() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::NEG_INFINITY.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert!(back.is_infinite() && back.is_sign_negative());

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_nan() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = f64::NAN.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert!(back.is_nan());

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_negative_zero() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let neg_zero = -0.0f64;
            let py_obj = neg_zero.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 0.0);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_pi() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = std::f64::consts::PI.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert!((back - std::f64::consts::PI).abs() < 1e-15);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_very_small() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let small = 1e-300f64;
            let py_obj = small.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, small);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_f64_very_large() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let large = 1e300f64;
            let py_obj = large.to_python(api).unwrap();
            let back = f64::from_python(py_obj, api).unwrap();
            assert_eq!(back, large);

            api.decref(py_obj);
        }
    }

    mod string_edge_cases {
        use super::*;

        #[test]
        #[serial]
        fn test_string_unicode_emoji() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "Hello 🎉🚀💻".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "Hello 🎉🚀💻");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_unicode_cjk() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "日本語 中文 한국어".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "日本語 中文 한국어");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_unicode_mixed() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "café résumé naïve".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "café résumé naïve");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_with_newlines() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "line1\nline2\nline3".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "line1\nline2\nline3");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_with_tabs() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "col1\tcol2\tcol3".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "col1\tcol2\tcol3");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_with_null_byte() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "before\0after".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "before\0after");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_special_chars() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = r#"quotes: "double" 'single' \backslash"#.to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, r#"quotes: "double" 'single' \backslash"#);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_long() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let long_str = "a".repeat(10000);
            let py_obj = long_str.as_str().to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, long_str);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_string_only_whitespace() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = "   \t\n\r   ".to_python(api).unwrap();
            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "   \t\n\r   ");

            api.decref(py_obj);
        }
    }

    mod option_edge_cases {
        use super::*;

        #[test]
        #[serial]
        fn test_option_some_f64() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some(std::f64::consts::PI).to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);

            let back = f64::from_python(py_obj, api).unwrap();
            assert!((back - std::f64::consts::PI).abs() < 1e-10);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_option_some_bool() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some(true).to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);
            assert_eq!(py_obj, api.py_true);
        }

        #[test]
        #[serial]
        fn test_option_some_string() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some("hello").to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);

            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "hello");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_option_none_f64() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Option::<f64>::None.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_none);
        }

        #[test]
        #[serial]
        fn test_option_none_bool() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Option::<bool>::None.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_none);
        }

        #[test]
        #[serial]
        fn test_option_none_string() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Option::<&str>::None.to_python(api).unwrap();
            assert_eq!(py_obj, api.py_none);
        }

        #[test]
        #[serial]
        fn test_option_some_zero_is_not_none() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some(0i64).to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);

            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 0);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_option_some_empty_string_is_not_none() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some("").to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);

            let back = String::from_python(py_obj, api).unwrap();
            assert_eq!(back, "");

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_option_some_false_is_not_none() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = Some(false).to_python(api).unwrap();
            assert_ne!(py_obj, api.py_none);
            assert_eq!(py_obj, api.py_false);
        }
    }

    mod i64_edge_cases {
        use super::*;

        #[test]
        #[serial]
        fn test_i64_one() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = 1i64.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, 1);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_negative_one_error_indicator_edge_case() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = (-1i64).to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, -1);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_large_positive() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let value = 9_000_000_000_000_000_000i64;
            let py_obj = value.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, value);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_large_negative() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let value = -9_000_000_000_000_000_000i64;
            let py_obj = value.to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, value);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_max_minus_one() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = (i64::MAX - 1).to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, i64::MAX - 1);

            api.decref(py_obj);
        }

        #[test]
        #[serial]
        fn test_i64_min_plus_one() {
            let Some(guard) = skip_if_no_python() else {
                return;
            };
            let api = guard.api();

            let py_obj = (i64::MIN + 1).to_python(api).unwrap();
            let back = i64::from_python(py_obj, api).unwrap();
            assert_eq!(back, i64::MIN + 1);

            api.decref(py_obj);
        }
    }

    #[test]
    #[serial]
    fn test_vec_to_python() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let vec = vec![1i64, 2, 3];
        let py_list = vec.to_python(api).unwrap();
        assert!(!py_list.is_null());
        assert!(api.list_check(py_list));
        assert_eq!(api.list_size(py_list), 3);

        let back: Vec<i64> = Vec::from_python(py_list, api).unwrap();
        assert_eq!(back, vec![1, 2, 3]);

        api.decref(py_list);
    }

    #[test]
    #[serial]
    fn test_hashmap_to_python() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let mut map = HashMap::new();
        map.insert("key".to_string(), 42i64);
        map.insert("another".to_string(), 100i64);

        let py_dict = map.to_python(api).unwrap();
        assert!(!py_dict.is_null());
        assert!(api.dict_check(py_dict));
        assert_eq!(api.dict_size(py_dict), 2);

        let back: HashMap<String, i64> = HashMap::from_python(py_dict, api).unwrap();
        assert_eq!(back.get("key"), Some(&42));
        assert_eq!(back.get("another"), Some(&100));

        api.decref(py_dict);
    }

    #[test]
    #[serial]
    fn test_nested_collections() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let nested = vec![vec![1i64, 2], vec![3, 4]];
        let py_obj = nested.to_python(api).unwrap();
        assert!(!py_obj.is_null());

        let back: Vec<Vec<i64>> = Vec::from_python(py_obj, api).unwrap();
        assert_eq!(back, vec![vec![1, 2], vec![3, 4]]);

        api.decref(py_obj);
    }

    #[test]
    #[serial]
    fn test_empty_vec() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let empty: Vec<i64> = vec![];
        let py_list = empty.to_python(api).unwrap();
        assert!(!py_list.is_null());
        assert!(api.list_check(py_list));
        assert_eq!(api.list_size(py_list), 0);

        let back: Vec<i64> = Vec::from_python(py_list, api).unwrap();
        assert!(back.is_empty());

        api.decref(py_list);
    }

    #[test]
    #[serial]
    fn test_empty_hashmap() {
        let Some(guard) = skip_if_no_python() else {
            return;
        };
        let api = guard.api();

        let empty: HashMap<String, i64> = HashMap::new();
        let py_dict = empty.to_python(api).unwrap();
        assert!(!py_dict.is_null());
        assert!(api.dict_check(py_dict));
        assert_eq!(api.dict_size(py_dict), 0);

        let back: HashMap<String, i64> = HashMap::from_python(py_dict, api).unwrap();
        assert!(back.is_empty());

        api.decref(py_dict);
    }
}
