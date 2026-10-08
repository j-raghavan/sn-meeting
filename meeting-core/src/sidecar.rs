//! Sidecar schema v1 (spec Data Model). Unknown fields survive a rewrite at every level (F3-FR6);
//! every known field is written, `null` included, except a segment's optional `flag`.
use crate::status::{FailedAt, Status};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
/// A sidecar is a few KB; a transcript of a long meeting a few hundred. Bigger is not ours.
pub const MAX_SIDECAR_BYTES: u64 = 8 * 1024 * 1024;

type Extra = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sidecar {
    pub schema: u32,
    pub id: String,
    /// RFC 3339 with the device's offset, e.g. `2026-10-07T21:17:06-07:00`. Kept verbatim.
    pub started_at: String,
    #[serde(default)]
    pub duration_ms: u64,
    pub status: Status,
    #[serde(default)]
    pub failed_at: Option<FailedAt>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_source: Option<TitleSource>,
    /// `null` until set per meeting or stamped from Settings at transcribe time (M4).
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub providers: Providers,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub sections: Vec<Section>,
    /// `null` only for a sidecar materialised from legacy audio (EC4).
    #[serde(default)]
    pub mic: Option<Mic>,
    #[serde(default)]
    pub audio: Option<Audio>,
    #[serde(default)]
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub transcript_file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitleSource {
    Auto,
    User,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Providers {
    #[serde(default)]
    pub transcribe: Option<ProviderRun>,
    #[serde(default)]
    pub summarise: Option<ProviderRun>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRun {
    pub provider: crate::keyfile::Provider,
    pub model: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub heading: String,
    pub body: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MicKind {
    Usb,
    Bluetooth,
}

/// `micJson` of `nativeBeginMeeting` (F2-FR4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mic {
    pub kind: MicKind,
    pub name: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `audioJson` of `nativeBeginMeeting` (F2-FR4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audio {
    pub codec: String,
    pub sample_rate_hz: u32,
    pub bitrate_bps: u32,
    pub channels: u8,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// A bare file name in the sidecar's own folder; validated by `check_segment_name`.
    pub file: String,
    pub start_offset_ms: u64,
    /// `null` = not yet closed (still recording, or recovery has not measured it).
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// `null` = not yet transcribed (F5-FR3).
    #[serde(default)]
    pub text: Option<String>,
    /// Additive to v1: set by recovery/close for a missing, empty or unreadable file (F3-FR5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<SegmentFlag>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentFlag {
    Missing,
    Empty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub done: bool,
    /// When extraction first saw it (F5-FR5); empty if a hand-edited sidecar lacks it.
    #[serde(default)]
    pub first_seen: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("sidecar unreadable: {0}")]
    Io(std::io::ErrorKind),
    #[error("sidecar is not valid JSON: {0}")]
    Parse(String),
    #[error("sidecar id {found:?} does not match its file name")]
    IdMismatch { found: String },
    #[error("unsafe file name in sidecar: {0:?}")]
    UnsafeName(String),
    #[error("sidecar schema {0} is newer than this app")]
    Schema(u32),
    #[error("invalid meeting id")]
    BadId,
}

/// Meeting ids: `meeting-YYYYMMDD-HHMMSS` plus an optional collision suffix `b`..`z` (F3-FR1).
pub fn is_valid_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("meeting-") else {
        return false;
    };
    let b = rest.as_bytes();
    let stamp_ok = b.len() >= 15
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..15].iter().all(u8::is_ascii_digit);
    stamp_ok && matches!(&b[15..], [] | [b'b'..=b'z'])
}

/// `<id>.m4a` (legacy) or `<id>-NN.m4a`, NN two or more digits from 01. Nothing else: no
/// separators, no `..`, so `dir.join(name)` stays in `dir` (Security, path traversal).
pub fn segment_number(id: &str, name: &str) -> Option<Option<u32>> {
    let stem = name.strip_suffix(".m4a")?.strip_prefix(id)?;
    if stem.is_empty() {
        return Some(None);
    }
    let nn = stem.strip_prefix('-')?;
    if nn.len() < 2 || !nn.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    nn.parse().ok().filter(|n| *n >= 1).map(Some)
}

pub fn segment_file_name(id: &str, number: u32) -> String {
    format!("{id}-{number:02}.m4a")
}
pub fn sidecar_file_name(id: &str) -> String {
    format!("{id}.json")
}
pub fn transcript_file_name(id: &str) -> String {
    format!("{id}.transcript.md")
}

impl Sidecar {
    /// Every file name the sidecar points at must be one of this meeting's own names.
    pub fn check_names(&self) -> Result<(), SidecarError> {
        for s in &self.segments {
            if segment_number(&self.id, &s.file).is_none() {
                return Err(SidecarError::UnsafeName(s.file.clone()));
            }
        }
        match &self.transcript_file {
            Some(t) if *t != transcript_file_name(&self.id) => {
                Err(SidecarError::UnsafeName(t.clone()))
            }
            _ => Ok(()),
        }
    }

    pub fn segment_paths(&self, dir: &Path) -> Vec<PathBuf> {
        self.segments.iter().map(|s| dir.join(&s.file)).collect()
    }
}

/// Read and validate `dir/<id>.json`. A sidecar of a newer schema is refused rather than
/// rewritten by an app that doesn't know its meaning.
pub fn read(dir: &Path, id: &str) -> Result<Sidecar, SidecarError> {
    if !is_valid_id(id) {
        return Err(SidecarError::BadId);
    }
    let path = dir.join(sidecar_file_name(id));
    let bytes = crate::atomic::read_capped(&path, MAX_SIDECAR_BYTES)
        .map_err(|e| SidecarError::Io(e.kind()))?;
    let sc: Sidecar =
        serde_json::from_slice(&bytes).map_err(|e| SidecarError::Parse(e.to_string()))?;
    if sc.schema != SCHEMA {
        return Err(SidecarError::Schema(sc.schema));
    }
    if sc.id != id {
        return Err(SidecarError::IdMismatch { found: sc.id });
    }
    sc.check_names()?;
    Ok(sc)
}

/// Atomic write of `dir/<id>.json` (F3-FR2). Only a valid id with its own file names is written.
pub fn write(dir: &Path, sc: &Sidecar) -> std::io::Result<()> {
    if !is_valid_id(&sc.id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid meeting id",
        ));
    }
    sc.check_names()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(sc).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(&dir.join(sidecar_file_name(&sc.id)), &bytes)
}
