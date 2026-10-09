//! F3-FR3 / F3-FR4: listing meetings across every library folder (legacy audio included), and
//! one meeting in full.
use super::{Found, LibError, Library};
use crate::sidecar::{self, Sidecar};
use crate::status::{FailedAt, Status};
use crate::tz;
use chrono::{DateTime, NaiveDateTime};
use serde::Serialize;

/// Where a listed meeting's details came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Sidecar,
    /// Audio with no sidecar (EC4): inferred from the file names.
    Legacy,
    /// A sidecar that can't be read: listed from its id and audio, never overwritten.
    Corrupt,
}

/// One row of `nativeListRecent` (F3-FR3, F6-FR1).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    pub id: String,
    pub started_at: String,
    pub started_at_ms: i64,
    /// `null` when not known (legacy or corrupt).
    pub duration_ms: Option<u64>,
    pub status: Status,
    pub failed_at: Option<FailedAt>,
    pub title: Option<String>,
    pub segment_count: usize,
    pub open_todo_count: usize,
    pub source: Source,
    pub folder: String,
    /// The meeting's cost so far (F16-FR5).
    pub spend: crate::spend::Total,
}

/// `nativeGetMeeting`: the whole sidecar, unknown fields included, plus absolute paths.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingView {
    pub meeting: Sidecar,
    pub source: Source,
    pub dir: String,
    pub segment_paths: Vec<String>,
    pub transcript_path: Option<String>,
    /// The meeting's cost so far (F16-FR5).
    pub spend: crate::spend::Total,
}

/// The local wall-clock time in an id (`meeting-YYYYMMDD-HHMMSS…`).
pub(crate) fn stamp_of(id: &str) -> Option<NaiveDateTime> {
    let stamp = id.strip_prefix("meeting-")?.get(..15)?;
    NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").ok()
}

impl Library<'_> {
    /// A sidecar for audio we have no readable sidecar for: `recorded`, one segment per file,
    /// started at the id's timestamp read as device local time (F3-FR4).
    pub(crate) fn infer(&self, id: &str, audio: &[(Option<u32>, String)]) -> Sidecar {
        let started = stamp_of(id).map(|n| tz::local_to_utc_ms(self.tz, n));
        let at = started.and_then(|ms| tz::local_at(self.tz, ms));
        Sidecar {
            started_at: at.map(|d| tz::rfc3339(&d)).unwrap_or_default(),
            status: Status::Recorded,
            segments: audio
                .iter()
                .map(|(_, f)| super::open_segment(f.clone(), 0))
                .collect(),
            ..sidecar::blank(id)
        }
    }

    /// The sidecar to show for a found meeting, and where it came from. Audio with no sidecar
    /// is a legacy meeting (EC4): listed, but no sidecar is written until the user acts on it.
    pub(crate) fn view_of(&self, id: &str, f: &Found) -> (Sidecar, Source) {
        if !f.has_sidecar {
            return (self.infer(id, &f.audio), Source::Legacy);
        }
        match sidecar::read(&f.dir, id) {
            Ok(sc) => (sc, Source::Sidecar),
            Err(_) => (self.infer(id, &f.audio), Source::Corrupt),
        }
    }

    /// Every listable meeting's sidecar (legacy and unreadable ones have no usage).
    pub fn sidecars(&self) -> Vec<Sidecar> {
        self.all().into_iter().map(|(_, sc)| sc).collect()
    }

    /// Every listable meeting with its sidecar, newest first (`startedAt`, then id).
    pub(crate) fn all(&self) -> Vec<(MeetingSummary, Sidecar)> {
        let mut v: Vec<(MeetingSummary, Sidecar)> = self
            .scan()
            .into_iter()
            .map(|(id, f)| {
                let (sc, source) = self.view_of(&id, &f);
                (self.summary(&id, &f, &sc, source), sc)
            })
            .collect();
        v.sort_by(|a, b| (b.0.started_at_ms, &b.0.id).cmp(&(a.0.started_at_ms, &a.0.id)));
        v
    }

    fn summary(&self, id: &str, f: &Found, sc: &Sidecar, source: Source) -> MeetingSummary {
        let from_stamp = || stamp_of(id).map_or(0, |n| tz::local_to_utc_ms(self.tz, n));
        let ms = DateTime::parse_from_rfc3339(&sc.started_at)
            .map(|d| d.timestamp_millis())
            .unwrap_or_else(|_| from_stamp());
        MeetingSummary {
            id: id.to_owned(),
            started_at: sc.started_at.clone(),
            started_at_ms: ms,
            duration_ms: (source == Source::Sidecar).then_some(sc.duration_ms),
            // The status machine's reading: `failed` without `failedAt` is failed(transcribing),
            // a stray `failedAt` is dropped (INV8), so the shell always has an action to offer.
            status: super::phase(sc).parts().0,
            failed_at: super::phase(sc).parts().1,
            title: sc.title.clone(),
            segment_count: sc.segments.len(),
            open_todo_count: sc.todos.iter().filter(|t| !t.done).count(),
            source,
            folder: f.dir.to_string_lossy().into_owned(),
            spend: crate::spend::meeting_total(sc),
        }
    }

    /// F3-FR3: the `n` newest meetings across the current and previous save folders.
    pub fn list_recent(&self, n: usize) -> Vec<MeetingSummary> {
        self.all().into_iter().take(n).map(|(s, _)| s).collect()
    }

    /// `nativeGetMeeting`. A sidecar that can't be read is an error here, with its reason.
    pub fn get(&self, id: &str) -> Result<MeetingView, LibError> {
        let f = self.find(id)?;
        let (mut sc, source) = if f.has_sidecar {
            (sidecar::read(&f.dir, id)?, Source::Sidecar)
        } else {
            (self.infer(id, &f.audio), Source::Legacy)
        };
        let p = super::phase(&sc);
        super::set_phase(&mut sc, p);
        let path = |n: &str| f.dir.join(n).to_string_lossy().into_owned();
        Ok(MeetingView {
            segment_paths: sc.segments.iter().map(|s| path(&s.file)).collect(),
            transcript_path: sc.transcript_file.as_deref().map(path),
            dir: f.dir.to_string_lossy().into_owned(),
            source,
            spend: crate::spend::meeting_total(&sc),
            meeting: sc,
        })
    }
}
