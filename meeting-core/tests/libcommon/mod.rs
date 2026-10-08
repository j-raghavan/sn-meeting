//! Shared fixtures for the library suites: a storage root, an app with a fixed time zone, and
//! the mic/audio JSON the shell sends.
#![allow(dead_code)] // each test binary uses a different subset

use chrono::FixedOffset;
use meeting::api::Core;
use meeting::app::App;
use meeting::library::Library;
use meeting::sidecar::{Audio, Mic, Sidecar};
use meeting::tz::FixedTz;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 2026-10-07T21:17:06-07:00.
pub const T0: i64 = 1_791_433_026_000;
pub const ID0: &str = "meeting-20261007-211706";
pub const PDT: i32 = -7 * 3600;

pub fn tz(offset_secs: i32) -> FixedTz {
    FixedTz(FixedOffset::east_opt(offset_secs).unwrap())
}

pub fn mic() -> Mic {
    serde_json::from_value(mic_json()).unwrap()
}

pub fn mic_json() -> Value {
    json!({"kind": "usb", "name": "USB-Audio - USB Audio"})
}

pub fn audio() -> Audio {
    serde_json::from_value(audio_json()).unwrap()
}

pub fn audio_json() -> Value {
    json!({"codec": "aac-lc", "sampleRateHz": 44100, "bitrateBps": 96000, "channels": 1})
}

/// A storage root and app data dir in temp folders.
pub struct Env {
    pub root: tempfile::TempDir,
    pub data: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Env {
        Env {
            root: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        }
    }

    pub fn app(&self) -> App {
        self.app_tz(PDT)
    }

    pub fn app_tz(&self, offset_secs: i32) -> App {
        let mut core = Core::device(self.data.path().to_str().unwrap());
        core.key_dir = self.data.path().join("no-keys");
        App::with_tz(core, self.root.path().into(), Arc::new(tz(offset_secs)))
    }

    /// `<root>/<rel>`, created.
    pub fn folder(&self, rel: &str) -> PathBuf {
        let p = self.root.path().join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub fn export(&self) -> PathBuf {
        self.root.path().join("EXPORT/sn-meeting")
    }
}

/// A library over `folders` in `tz` (no app).
pub fn library<'a>(folders: &[&Path], tz: &'a FixedTz) -> Library<'a> {
    Library {
        folders: folders.iter().map(|p| p.to_path_buf()).collect(),
        tz,
    }
}

pub fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("{e}: {s}"))
}

pub fn read_sidecar(dir: &Path, id: &str) -> Sidecar {
    meeting::sidecar::read(dir, id).unwrap()
}

pub fn begin(app: &App, at: i64) -> Value {
    parse(&app.begin_json(at, &mic_json().to_string(), &audio_json().to_string()))
}
