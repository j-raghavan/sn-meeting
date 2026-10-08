//! Meetings core (`libmeeting.so`).
pub mod api;
pub mod providers;
pub mod runtime;

#[cfg(feature = "jni-bridge")]
mod jni;

/// The crate version (F1-FR5).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
