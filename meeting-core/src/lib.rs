//! Meetings core (`libmeeting.so`).
pub mod api;
pub mod app;
pub mod atomic;
pub mod calendar;
pub mod chunk;
pub mod ink;
pub mod jobs;
pub mod keyfile;
pub mod library;
pub mod marks;
pub mod minutes;
pub mod mp4;
pub mod notes;
pub mod notes_text;
pub mod pipeline;
pub mod providers;
pub mod rates;
pub mod runtime;
pub mod scrub;
pub mod search;
pub mod segment;
pub mod settings;
pub mod sidecar;
pub mod spend;
pub mod status;
pub mod summarise;
pub mod tags;
pub mod templates;
pub mod tz;
pub mod update;

#[cfg(feature = "jni-bridge")]
mod jni;

/// The crate version (F1-FR5).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
