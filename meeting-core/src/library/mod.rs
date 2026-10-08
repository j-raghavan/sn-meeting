//! F3: the meeting library. Folders are scanned on every call; there is no index (ADR D5). Every
//! call runs on the shell's single CoreJobs thread, which serialises writes.
use crate::segment::{self, SegmentPlan};
use crate::sidecar::{self, Audio, Mic, Segment, Sidecar, SidecarError};
use crate::status::Status;
use crate::tz::{self, LocalTz};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_MIC_NAME: usize = 200;
const MAX_CODEC: usize = 32;

/// The library's view of the world: its folders (current first) and the device time zone.
pub struct Library<'a> {
    pub folders: Vec<PathBuf>,
    pub tz: &'a dyn LocalTz,
}

/// What is running in this process right now (owned by `App`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Liveness {
    /// The meeting being recorded, from `nativeBeginMeeting` until it is finished.
    pub recording: Option<String>,
    /// Meetings with a transcribe or summarise job in flight (M4 registers them).
    pub jobs: HashSet<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LibError {
    #[error("invalid meeting id")]
    BadId,
    #[error("meeting not found")]
    NotFound,
    #[error(transparent)]
    Sidecar(#[from] SidecarError),
    #[error("{0}")]
    Invalid(String),
    #[error("storage: {0}")]
    Io(std::io::ErrorKind),
}

impl From<std::io::Error> for LibError {
    fn from(e: std::io::Error) -> Self {
        LibError::Io(e.kind())
    }
}

fn invalid(msg: impl Into<String>) -> LibError {
    LibError::Invalid(msg.into())
}

/// `nativeBeginMeeting`'s reply.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Begun {
    pub id: String,
    pub first_segment_path: String,
    pub plan: SegmentPlan,
}

/// One meeting's files in one folder.
#[allow(dead_code)] // read once segments and listings arrive (F3-FR2, F3-FR3)
pub(crate) struct Found {
    pub(crate) dir: PathBuf,
    pub(crate) has_sidecar: bool,
    /// Every audio file of this id in `dir`, `(segment number, name)`, sorted.
    pub(crate) audio: Vec<(Option<u32>, String)>,
}

/// The id a file name belongs to, and whether it is that id's sidecar. Anything else (other
/// apps' files, `.tmp`, the transcript) is not a meeting file.
fn owner_of(name: &str) -> Option<(String, bool)> {
    let rest = name.strip_prefix("meeting-")?;
    let suffixed = rest
        .as_bytes()
        .get(15)
        .is_some_and(|c| c.is_ascii_lowercase());
    let id = name.get(.."meeting-".len() + if suffixed { 16 } else { 15 })?;
    if !sidecar::is_valid_id(id) {
        return None;
    }
    if name == sidecar::sidecar_file_name(id) {
        return Some((id.to_owned(), true));
    }
    sidecar::segment_number(id, name).map(|_| (id.to_owned(), false))
}

impl Library<'_> {
    pub(crate) fn current(&self) -> &Path {
        &self.folders[0]
    }

    /// Every meeting in every folder; when an id is in two folders, the earlier folder wins.
    pub(crate) fn scan(&self) -> BTreeMap<String, Found> {
        let mut out: BTreeMap<String, Found> = BTreeMap::new();
        for dir in &self.folders {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue; // a folder that is gone or unreadable lists nothing
            };
            let mut here: BTreeMap<String, Found> = BTreeMap::new();
            for e in entries.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some((id, is_sidecar)) = owner_of(&name) else {
                    continue;
                };
                let slot = here.entry(id.clone()).or_insert_with(|| Found {
                    dir: dir.clone(),
                    has_sidecar: false,
                    audio: Vec::new(),
                });
                if is_sidecar {
                    slot.has_sidecar = true;
                } else if let Some(n) = sidecar::segment_number(&id, &name) {
                    slot.audio.push((n, name));
                }
            }
            for (id, mut f) in here {
                f.audio.sort();
                out.entry(id).or_insert(f);
            }
        }
        out
    }

    /// F3-FR1: create the sidecar (`recording`) in the current folder and return the id, the
    /// first segment's path and the recorder's plan. `started_at_ms` is the shell's wall clock
    /// at Record. A second meeting in the same second gets a `b`..`z` suffix; ids already used
    /// by any file in any library folder (sidecar or legacy audio) are skipped.
    pub fn begin(
        &self,
        started_at_ms: i64,
        mic: Mic,
        audio: Audio,
        debug_segment_ms: Option<u64>,
    ) -> Result<Begun, LibError> {
        check_mic(&mic)?;
        check_audio(&audio)?;
        let plan = segment::plan(audio.bitrate_bps, debug_segment_ms)
            .ok_or_else(|| invalid("bitrateBps must be > 0"))?;
        // Before 1970 is a broken clock, not a meeting; after 9999 has no id.
        let local = tz::local_at(self.tz, started_at_ms)
            .filter(|_| started_at_ms >= 0)
            .ok_or_else(|| invalid("startedAtMs is out of range"))?;
        let base = local.format("meeting-%Y%m%d-%H%M%S").to_string();
        if !sidecar::is_valid_id(&base) {
            return Err(invalid("startedAtMs is out of range"));
        }
        let taken = self.scan();
        let id = std::iter::once(String::new())
            .chain(('b'..='z').map(String::from))
            .map(|s| format!("{base}{s}"))
            .find(|id| !taken.contains_key(id))
            .ok_or_else(|| invalid("too many meetings started in one second"))?;
        std::fs::create_dir_all(self.current())?;
        let first = sidecar::segment_file_name(&id, 1);
        let sc = Sidecar {
            started_at: tz::rfc3339(&local),
            status: Status::Recording,
            mic: Some(mic),
            audio: Some(audio),
            segments: vec![open_segment(first.clone(), 0)],
            ..sidecar::blank(&id)
        };
        sidecar::write(self.current(), &sc)?;
        let path = self.current().join(first);
        Ok(Begun {
            id,
            first_segment_path: path.to_string_lossy().into_owned(),
            plan,
        })
    }
}

pub(crate) fn open_segment(file: String, start_offset_ms: u64) -> Segment {
    Segment {
        file,
        start_offset_ms,
        duration_ms: None,
        text: None,
        flag: None,
        extra: Default::default(),
    }
}

/// F2-FR4's `micJson`: a kind and the device's name.
fn check_mic(m: &Mic) -> Result<(), LibError> {
    let n = m.name.trim();
    if n.is_empty() || n.chars().count() > MAX_MIC_NAME || n.chars().any(char::is_control) {
        return Err(invalid("mic name is invalid"));
    }
    Ok(())
}

/// F2-FR4's `audioJson`: AAC settings the recorder actually used.
fn check_audio(a: &Audio) -> Result<(), LibError> {
    let codec_ok = !a.codec.is_empty()
        && a.codec.len() <= MAX_CODEC
        && a.codec
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !codec_ok {
        return Err(invalid("codec is invalid"));
    }
    if a.sample_rate_hz == 0 {
        return Err(invalid("sampleRateHz must be > 0"));
    }
    if !(1..=2).contains(&a.channels) {
        return Err(invalid("channels must be 1 or 2"));
    }
    Ok(())
}
