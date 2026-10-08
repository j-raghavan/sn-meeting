//! The JNI-shaped API: strings in, JSON out, never a panic. `jni.rs` only marshals.
use serde_json::json;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Run `f`, turning a panic into `{"error": "internal error"}` (INV6, NFR4).
pub fn guarded(f: impl FnOnce() -> String) -> String {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| error_json("internal error"))
}

/// The error envelope every export returns instead of throwing.
pub fn error_json(msg: &str) -> String {
    json!({ "error": msg }).to_string()
}

/// `nativeVersion`: the crate version, as a plain string (F1-FR5).
pub fn version_json() -> String {
    crate::version().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_the_error_envelope() {
        assert_eq!(guarded(|| panic!("boom")), r#"{"error":"internal error"}"#);
        assert_eq!(guarded(|| "fine".to_owned()), "fine");
    }

    #[test]
    fn error_json_escapes() {
        assert_eq!(error_json("a\"b\n"), r#"{"error":"a\"b\n"}"#);
    }

    #[test]
    fn version_json_is_the_version() {
        assert_eq!(version_json(), crate::version());
        assert_eq!(crate::version(), "0.1.0");
    }
}
