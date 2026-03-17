//! Many call sites (trait impls like `From<PythonException>`, deeply nested
//! helpers) don't have a `&Ruby` in scope and cannot easily thread one through.
//! These helpers centralise the `Ruby::get()` call so each caller can simply
//! write `runtime_error()` instead of repeating the boilerplate.
//!
//! # Panics
//!
//! Every function in this module calls `Ruby::get().expect(…)` and will panic
//! if invoked from a non-Ruby thread. This is intentional — it mirrors the
//! behaviour of the deprecated `magnus::exception::*()` functions, and all
//! code paths that reach these helpers originate from Ruby callbacks registered
//! via magnus, where `Ruby::get()` is guaranteed to succeed.
use magnus::ExceptionClass;

/// Returns Ruby's `RuntimeError` exception class.
pub(crate) fn runtime_error() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_runtime_error()
}

/// Returns Ruby's `TypeError` exception class.
pub(crate) fn type_error() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_type_error()
}

/// Returns Ruby's `ArgumentError` exception class.
pub(crate) fn arg_error() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_arg_error()
}

/// Returns Ruby's `SyntaxError` exception class.
pub(crate) fn syntax_error() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_syntax_error()
}

/// Returns Ruby's base `Exception` exception class.
pub(crate) fn exception() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_exception()
}
