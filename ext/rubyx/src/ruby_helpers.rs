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
use magnus::{ExceptionClass, Module};

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

/// Returns Ruby's `NoMethodError` exception class.
pub(crate) fn no_method_error() -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    ruby.exception_no_method_error()
}

/// Look up a Rubyx error class by Python exception kind.
/// Falls back to `Rubyx::PythonError` for unrecognized kinds,
/// and `RuntimeError` if the Rubyx module isn't available.
pub(crate) fn rubyx_exception_class(kind: &str) -> ExceptionClass {
    let ruby = magnus::Ruby::get().expect("must be called from Ruby thread");
    let rubyx = match ruby.define_module("Rubyx") {
        Ok(m) => m,
        Err(_) => return ruby.exception_runtime_error(),
    };

    let class_name = match kind {
        "KeyError" => "KeyError",
        "IndexError" => "IndexError",
        "ValueError" => "ValueError",
        "AttributeError" => "AttributeError",
        "TypeError" => "TypeError",
        "ImportError" | "ModuleNotFoundError" => "ImportError",
        _ => "PythonError",
    };

    rubyx
        .const_get::<_, ExceptionClass>(class_name)
        .unwrap_or_else(|_| ruby.exception_runtime_error())
}
