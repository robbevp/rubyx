use crate::python_api::PythonApi;
use crate::python_ffi::PyObject;

/// RAII guard that decrefs a PyObject when dropped.
pub(crate) struct PyGuard<'a> {
    obj: *mut PyObject,
    api: &'a PythonApi,
}
impl<'a> PyGuard<'a> {
    pub(crate) fn new(obj: *mut PyObject, api: &'a PythonApi) -> Option<Self> {
        if obj.is_null() {
            None
        } else {
            Some(Self { obj, api })
        }
    }
    pub(crate) fn ptr(&self) -> *mut PyObject {
        self.obj
    }
}
impl Drop for PyGuard<'_> {
    fn drop(&mut self) {
        self.api.decref(self.obj);
    }
}
