use std::ffi::c_void;

pub type PyObject = c_void;
#[allow(non_camel_case_types)]
pub type Py_ssize_t = isize;

#[repr(C)]
pub struct PyThreadState {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum PyGILState {
    Locked = 0,
    Unlocked = 1,
}
