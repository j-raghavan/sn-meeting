//! Sidecar schema v1 (spec Data Model). Unknown fields survive a rewrite at every level (F3-FR6);
//! every known field is written, `null` included, except a segment's optional `flag`.
use crate::status::{FailedAt, Status};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
/// A sidecar is a few KB; a transcript of a long meeting a few hundred. Bigger is not ours.
pub const MAX_SIDECAR_BYTES: u64 = 8 * 1024 * 1024;

/// Unknown fields, kept on rewrite. Their values are kept; how a number is written may change
/// (`1e2` is written back as `100.0`), since serde_json reads it as a float.
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
    /// F16-FR2: one record per successful provider call, oldest first. Optional (schema 1):
    /// absent means none, and an empty list is not written.
    /// An entry that isn't a record is kept as written (F16-FR8).
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::spend::lenient_usage"
    )]
    pub usage: Vec<crate::spend::UsageEntry>,
    /// F15: the near-live chunks, in index order. Only meetings recorded with the near-live
    /// recorder have them (F15-FR10); absent means none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<Chunk>,
    /// F15-FR5: the near-live transcription fixed at Record; absent for the Standard recorder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// F20-FR3: the meeting's tags, normalised (F20-FR2), each the AI's or the user's.
    /// Optional (schema 1): absent means none; an entry that isn't a tag is dropped on read.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "lenient_tags"
    )]
    pub tags: Vec<Tag>,
    /// F20-FR3: tags the user removed, so "Summarise again" doesn't bring them back.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "lenient_names"
    )]
    pub tags_user_removed: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// F20-FR3: one tag on a meeting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub source: TagSource,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Tag {
    pub fn new(name: &str, source: TagSource) -> Tag {
        Tag {
            name: name.to_owned(),
            source,
            extra: Extra::new(),
        }
    }
}

/// Who put a tag on: the summarise call, or the user (kept through "Summarise again").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagSource {
    Ai,
    User,
}

/// The readable tags, normalised on the way in; a hand-edited entry that isn't a tag, or a
/// repeat, is dropped rather than making the whole sidecar unreadable.
fn lenient_tags<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Tag>, D::Error> {
    let Value::Array(items) = Value::deserialize(d)? else {
        return Ok(Vec::new());
    };
    let mut out: Vec<Tag> = Vec::new();
    for v in items {
        let Ok(mut t) = serde_json::from_value::<Tag>(v) else {
            continue;
        };
        let Some(name) = crate::tags::normalise(&t.name) else {
            continue;
        };
        if out.iter().all(|o| o.name != name) {
            t.name = name;
            out.push(t);
        }
    }
    Ok(out)
}

/// The readable names in a list of names, normalised, without repeats.
fn lenient_names<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let Value::Array(items) = Value::deserialize(d)? else {
        return Ok(Vec::new());
    };
    Ok(crate::tags::normalise_all(
        items.iter().filter_map(Value::as_str),
    ))
}

/// F15: one ~30 s stretch of the recording (spec Data Model, `chunks`). Times are
/// recording-clock ms (INV13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub index: u32,
    /// The segment it lies in (0-based): a chunk never spans a segment boundary (F15-FR4).
    pub seg_index: usize,
    pub start_offset_ms: u64,
    pub duration_ms: u64,
    /// How much of its start repeats the previous chunk's end (0, or about 1 s after a hard
    /// cut); removed when the texts are joined (F15-FR6).
    #[serde(default)]
    pub overlap_ms: u64,
    pub state: ChunkState,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    /// The last failure, user-worded and scrubbed (Security).
    #[serde(default)]
    pub last_error: Option<String>,
    /// Its audio above the pause threshold (F15-FR5's empty-reply rule); absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkState {
    Pending,
    Done,
    /// Its audio is gone and can't be re-cut (EC7's rule): "(audio missing)", never sent.
    Missing,
}

/// F15-FR5: who transcribes the chunks, fixed when recording began.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Live {
    pub provider: crate::keyfile::Provider,
    /// `null` until the account's listing names one (the auto-pick, F5-FR12).
    #[serde(default)]
    pub model: Option<String>,
    /// Why the near-live lane is paused (user-safe), or `null` while it runs.
    #[serde(default)]
    pub paused: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitleSource {
    Auto,
    User,
    /// F18-FR12: the linked event's title; extraction may replace it, as `auto`.
    Event,
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
    /// F22-FR1: the device's own mic (`"builtin"`).
    Builtin,
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

/// `<id>.m4a` (legacy) or `<id>-NN.m4a`, NN from 01 as `segment_file_name` writes it. Nothing
/// else: no separators, no `..`, so `dir.join(name)` stays in `dir` (Security, path traversal).
pub fn segment_number(id: &str, name: &str) -> Option<Option<u32>> {
    let stem = name.strip_suffix(".m4a")?.strip_prefix(id)?;
    if stem.is_empty() {
        return Some(None);
    }
    let nn = stem.strip_prefix('-')?;
    if nn.len() < 2 || !nn.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Canonical only ("-01", "-100"; not "-001"), so one number has one name.
    nn.parse()
        .ok()
        .filter(|n| *n >= 1 && segment_file_name(id, *n) == name)
        .map(Some)
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

/// A sidecar for `id` with every field at its empty value; callers fill in the rest.
pub fn blank(id: &str) -> Sidecar {
    Sidecar {
        schema: SCHEMA,
        id: id.to_owned(),
        started_at: String::new(),
        duration_ms: 0,
        status: Status::Recorded,
        failed_at: None,
        title: None,
        title_source: None,
        template_id: None,
        providers: Providers::default(),
        summary: None,
        decisions: Vec::new(),
        sections: Vec::new(),
        mic: None,
        audio: None,
        segments: Vec::new(),
        transcript_file: None,
        error: None,
        note: None,
        todos: Vec::new(),
        usage: Vec::new(),
        chunks: Vec::new(),
        live: None,
        tags: Vec::new(),
        tags_user_removed: Vec::new(),
        extra: Extra::new(),
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
