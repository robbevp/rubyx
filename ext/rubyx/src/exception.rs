use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum PythonException {
    #[error("{kind}: {message}")]
    Exception {
        kind: String,    // e.g., "TypeError"
        message: String, // e.g., "expected int, got str"
        traceback: Option<String>,
    },

    #[error("SyntaxError at line {line}: {message}")]
    SyntaxError {
        message: String,
        filename: String,
        line: usize,
        offset: usize,
    },
}
impl PythonException {
    /// Format the exception as a message for magnus::Error.
    ///
    /// Separate from Display (thiserror) because magnus needs a different format:
    /// Exception includes traceback, SyntaxError includes filename and offset.
    pub(crate) fn to_magnus_message(&self) -> String {
        match self {
            PythonException::Exception {
                kind,
                message,
                traceback,
            } => {
                if let Some(tb) = traceback {
                    format!("{}: {}\n{}", kind, message, tb)
                } else {
                    format!("{}: {}", kind, message)
                }
            }
            PythonException::SyntaxError {
                message,
                filename,
                line,
                offset,
            } => {
                format!(
                    "SyntaxError: {} ({}:{}:{})",
                    message, filename, line, offset
                )
            }
        }
    }
}
impl From<PythonException> for magnus::Error {
    fn from(e: PythonException) -> Self {
        let (class, msg) = match &e {
            PythonException::Exception { .. } => {
                (crate::ruby_helpers::runtime_error(), e.to_magnus_message())
            }
            PythonException::SyntaxError { .. } => {
                (crate::ruby_helpers::syntax_error(), e.to_magnus_message())
            }
        };
        magnus::Error::new(class, msg)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // ===== Display (thiserror) tests =====
    #[test]
    fn test_exception_display_format() {
        let exc = PythonException::Exception {
            kind: "TypeError".into(),
            message: "expected int".into(),
            traceback: None,
        };
        assert_eq!(exc.to_string(), "TypeError: expected int");
    }
    #[test]
    fn test_exception_display_ignores_traceback() {
        // Display (thiserror) format does NOT include traceback
        let exc = PythonException::Exception {
            kind: "ValueError".into(),
            message: "bad value".into(),
            traceback: Some("  File \"test.py\", line 1".into()),
        };
        assert_eq!(exc.to_string(), "ValueError: bad value");
    }
    #[test]
    fn test_syntax_error_display_format() {
        let exc = PythonException::SyntaxError {
            message: "invalid syntax".into(),
            filename: "test.py".into(),
            line: 42,
            offset: 5,
        };
        assert_eq!(exc.to_string(), "SyntaxError at line 42: invalid syntax");
    }
    // ===== Magnus message formatting tests =====
    // We test to_magnus_message() directly because magnus::Error::new()
    // requires a Ruby VM thread, which cargo test doesn't guarantee.
    #[test]
    fn test_magnus_message_exception_no_traceback() {
        let exc = PythonException::Exception {
            kind: "TypeError".into(),
            message: "expected int".into(),
            traceback: None,
        };
        assert_eq!(exc.to_magnus_message(), "TypeError: expected int");
    }
    #[test]
    fn test_magnus_message_exception_with_traceback() {
        let exc = PythonException::Exception {
            kind: "ValueError".into(),
            message: "invalid value".into(),
            traceback: Some("  File \"test.py\", line 1\n    x = bad".into()),
        };
        let msg = exc.to_magnus_message();
        assert_eq!(
            msg,
            "ValueError: invalid value\n  File \"test.py\", line 1\n    x = bad"
        );
    }
    #[test]
    fn test_magnus_message_syntax_error() {
        let exc = PythonException::SyntaxError {
            message: "invalid syntax".into(),
            filename: "test.py".into(),
            line: 42,
            offset: 5,
        };
        assert_eq!(
            exc.to_magnus_message(),
            "SyntaxError: invalid syntax (test.py:42:5)"
        );
    }
    #[test]
    fn test_magnus_message_syntax_error_string_source() {
        let exc = PythonException::SyntaxError {
            message: "unexpected EOF".into(),
            filename: "<string>".into(),
            line: 1,
            offset: 0,
        };
        assert_eq!(
            exc.to_magnus_message(),
            "SyntaxError: unexpected EOF (<string>:1:0)"
        );
    }
    // ===== Enum trait tests =====
    #[test]
    fn test_exception_clone_preserves_fields() {
        let exc = PythonException::Exception {
            kind: "TypeError".into(),
            message: "test".into(),
            traceback: Some("tb".into()),
        };
        let cloned = exc.clone();
        assert_eq!(exc.to_string(), cloned.to_string());
        assert_eq!(exc.to_magnus_message(), cloned.to_magnus_message());
    }
    #[test]
    fn test_exception_debug_contains_fields() {
        let exc = PythonException::Exception {
            kind: "TypeError".into(),
            message: "test".into(),
            traceback: None,
        };
        let debug = format!("{:?}", exc);
        assert!(debug.contains("TypeError"));
        assert!(debug.contains("test"));
    }
    #[test]
    fn test_exception_implements_std_error() {
        let exc = PythonException::Exception {
            kind: "TypeError".into(),
            message: "test".into(),
            traceback: None,
        };
        // PythonException implements std::error::Error via thiserror
        let err: &dyn std::error::Error = &exc;
        assert!(err.to_string().contains("TypeError"));
    }
}
