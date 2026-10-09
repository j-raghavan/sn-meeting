//! F3-FR5: crash recovery, run on every launch and resume. Idempotent, and it never deletes.
use super::{Library, Liveness, phase, set_phase};
use crate::sidecar::{self, SegmentFlag, Sidecar};
use crate::status::{self, Event, Status};
use serde::Serialize;
use std::path::Path;

/// `nativeRecover`'s reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    /// Meetings moved out of `recording` (to `recorded`) or `transcribing` (to failed).
    pub recovered: Vec<String>,
    /// Open segments of recorded meetings: the shell measures each file and calls
    /// `nativeCloseSegment(id, index, ms)`, with 0 if it can't be read.
    pub measure: Vec<Measure>,
    /// Segments newly found missing or empty: flagged (an open one closed at 0 ms), never deleted.
    pub flagged: Vec<Flagged>,
    /// Recorded meetings with no segment at all, newly marked "No audio was recorded".
    /// Additive: omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub no_audio: Vec<String>,
    /// F15-FR9 / F15-AC8: near-live meetings recovered from `recording` with chunks still to
    /// send (and sending allowed): the shell runs their Stop job, so the transcript catches up
    /// and the minutes fill in. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resume: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    pub id: String,
    pub index: usize,
    pub path: String,
    /// F15-FR8: the segment's finalised chunk files, in order. If the segment file can't be
    /// read (its muxer was killed before it wrote the index), the shell rebuilds it from these
    /// by remuxing their AAC frames, then measures it. Omitted when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<super::Piece>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flagged {
    pub id: String,
    pub index: usize,
    pub flag: SegmentFlag,
}

pub(crate) const INTERRUPTED: &str = "Transcription was interrupted";

impl Library<'_> {
    /// F3-FR5. `recording` with no live recording becomes `recorded`; `transcribing` with no
    /// job in flight becomes failed(transcribing), keeping every segment's text so a retry
    /// resumes. Recorded meetings' segments are then checked: a missing or empty file is
    /// flagged, an open one is listed for the shell to measure. A meeting past recorded only
    /// has its open, non-empty segments listed (D3: legacy audio a Transcribe tap gave a
    /// sidecar before it was measured). The live meeting and running jobs keep their status. Unreadable sidecars are not touched; legacy audio (no sidecar) is
    /// only listed for measuring.
    /// `chunks` is `<filesDir>/chunks`, where a near-live meeting's chunk files are (F15-FR8).
    pub fn recover(&self, live: &Liveness, chunks: Option<&Path>) -> Recovery {
        let mut r = Recovery::default();
        for (id, f) in self.scan() {
            if !f.has_sidecar {
                // D3: legacy audio is measured too; closing it writes its first sidecar.
                r.measure.extend(legacy_measures(&id, &f));
                continue;
            }
            let Ok(mut sc) = sidecar::read(&f.dir, &id) else {
                continue;
            };
            let event = match sc.status {
                Status::Recording if live.recording.as_deref() != Some(id.as_str()) => {
                    Some(Event::RecoverNoService)
                }
                Status::Transcribing if !live.jobs.contains(&id) => Some(Event::RecoverNoJob),
                _ => None,
            };
            // A hand-edited pair (failed without failedAt, failedAt on another status) is
            // rewritten as the status machine reads it (INV8).
            let canonical = phase(&sc).parts() != (sc.status, sc.failed_at);
            if canonical {
                let p = phase(&sc);
                set_phase(&mut sc, p);
            }
            let mut moved = false;
            if let Some(ev) = event
                && let Ok(p) = status::next(phase(&sc), ev)
            {
                set_phase(&mut sc, p);
                if ev == Event::RecoverNoJob {
                    sc.error = Some(INTERRUPTED.into());
                }
                moved = true;
            }
            let empty = sc.status == Status::Recorded
                && sc.segments.is_empty()
                && sc.error.as_deref() != Some(crate::pipeline::NO_AUDIO);
            if empty {
                sc.error = Some(crate::pipeline::NO_AUDIO.into());
            }
            let (flagged, measure) = match sc.status {
                Status::Recorded => check_segments(&id, &f.dir, &mut sc),
                Status::Recording => (Vec::new(), Vec::new()), // the live recording's own
                _ => (Vec::new(), unmeasured(&id, &f.dir, &sc)),
            };
            // S13: a crash leaves durationMs 0; it lasts at least to its last closed segment.
            let before = sc.duration_ms;
            if sc.status != Status::Recording {
                super::extend_duration(&mut sc);
            }
            let longer = sc.duration_ms != before;
            let changed = moved || canonical || empty || longer || !flagged.is_empty();
            if changed && sidecar::write(&f.dir, &sc).is_err() {
                continue; // not recovered after all; the next launch tries again
            }
            if moved {
                r.recovered.push(id.clone());
                let sending = sc.live.as_ref().is_some_and(|l| l.paused.is_none());
                let pending = sc
                    .chunks
                    .iter()
                    .any(|c| c.state == crate::sidecar::ChunkState::Pending);
                if sc.status == Status::Recorded && sending && pending {
                    r.resume.push(id.clone());
                }
            }
            if empty {
                r.no_audio.push(id);
            }
            r.flagged.extend(flagged);
            r.measure.extend(measure.into_iter().map(|mut m| {
                if let Some(root) = chunks {
                    m.chunks = super::chunks::pieces(root, &sc, m.index);
                }
                m
            }));
        }
        r
    }
}

/// Every non-empty audio file of a meeting with no sidecar, by its index in the inferred one.
fn legacy_measures(id: &str, f: &super::Found) -> Vec<Measure> {
    f.audio
        .iter()
        .enumerate()
        .filter_map(|(index, (_, name))| {
            let path = f.dir.join(name);
            let len = std::fs::metadata(&path).map_or(0, |m| m.len());
            (len > 0).then(|| Measure {
                id: id.to_owned(),
                index,
                path: path.to_string_lossy().into_owned(),
                chunks: Vec::new(),
            })
        })
        .collect()
}

/// The open segments of a meeting past recorded whose file has audio, for measuring; nothing
/// is flagged here (its segments were checked when it was recorded).
fn unmeasured(id: &str, dir: &Path, sc: &Sidecar) -> Vec<Measure> {
    sc.segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.duration_ms.is_none())
        .filter_map(|(index, s)| {
            let path = dir.join(&s.file);
            let len = std::fs::metadata(&path).map_or(0, |m| m.len());
            (len > 0).then(|| Measure {
                id: id.to_owned(),
                index,
                path: path.to_string_lossy().into_owned(),
                chunks: Vec::new(),
            })
        })
        .collect()
}

/// Flag missing (not found) or empty segment files; list open, present ones for measuring.
fn check_segments(id: &str, dir: &Path, sc: &mut Sidecar) -> (Vec<Flagged>, Vec<Measure>) {
    let (mut flagged, mut measure) = (Vec::new(), Vec::new());
    for (index, s) in sc.segments.iter_mut().enumerate() {
        let path = dir.join(&s.file);
        let found = match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(SegmentFlag::Missing),
            // Can't be checked now (permissions, a link loop): neither flagged nor measured.
            Err(_) => continue,
            Ok(m) if m.len() == 0 => Some(SegmentFlag::Empty),
            Ok(_) => None,
        };
        match found {
            Some(flag) if s.flag != Some(flag) => {
                s.flag = Some(flag);
                s.duration_ms.get_or_insert(0);
                flagged.push(Flagged {
                    id: id.to_owned(),
                    index,
                    flag,
                });
            }
            Some(_) => {}
            None if s.duration_ms.is_none() => measure.push(Measure {
                id: id.to_owned(),
                index,
                path: path.to_string_lossy().into_owned(),
                chunks: Vec::new(),
            }),
            None => {}
        }
    }
    (flagged, measure)
}
