//! Meetings core (`libmeeting.so`).
pub mod api;
pub mod app;
pub mod atomic;
pub mod keyfile;
pub mod providers;
pub mod runtime;
pub mod scrub;
pub mod segment;
pub mod settings;
pub mod sidecar;
pub mod status;

#[cfg(feature = "jni-bridge")]
mod jni;

/// The crate version (F1-FR5).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
