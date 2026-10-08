//! F5-FR2 / FR3: the transcribe job, then summarise (`summary.rs`, F5-FR8). Every sidecar change is one short read-modify-write under
//! the core write lock (`Library::update`); no sidecar copy is held across an await, so an edit
//! made while a request is in flight (a to-do ticked, a rename) is never overwritten.
use crate::jobs::{self, Cancel};
use crate::keyfile::ProviderKey;
use crate::library::{self, LibError, Library};
use crate::providers::{self, AudioIn, CallFail, Endpoints, Http};
use crate::sidecar::{ProviderRun, SegmentFlag, Sidecar};
use crate::status::{self, Event, Phase};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

mod summary;
mod transcript;
pub use summary::{context, summarise};
pub use transcript::rewrite as rewrite_transcript;

/// A meeting with no segment at all (a recording that never produced a file).
pub const NO_AUDIO: &str = "No audio was recorded";

/// The text stored for a segment whose audio file is gone (EC7). Transcription carries on.
pub const AUDIO_MISSING: &str = "(audio missing)";

/// OpenAI's per-file cap (C3). The recorder's plan keeps segments far below it (F2-FR5), so a
/// bigger file is not sent at all.
pub const MAX_AUDIO_BYTES: u64 = 25 * 1024 * 1024;

/// Everything a job needs, taken when it starts (F5-FR10): a Settings change or a key-file
/// edit while it runs does not reach it.
#[derive(Debug)]
pub struct JobPlan {
    pub http: Http,
    pub endpoints: Endpoints,
    /// The transcribe provider's key and model; `None` for a summarise-only job.
    pub transcribe: Option<(ProviderKey, String)>,
    /// The summarise provider's key and model.
    pub summarise: Option<(ProviderKey, String)>,
    /// Why there is no summarise provider, for a transcription that ends without a summary.
    pub summarise_problem: Option<String>,
    /// The transcription model for the one retry when the account can't use the chosen one
    /// (D1).
    pub transcribe_fallback: Option<String>,
    /// The summarise provider's named default, for the one retry when the model is rejected
    /// (F4-FR3).
    pub summarise_fallback: Option<String>,
    /// The template stamped on a meeting that has none yet (F11-FR6).
    pub default_template: String,
    /// The effective templates at job start (F11-FR3).
    pub templates: Vec<crate::templates::Template>,
    pub budgets: jobs::Budgets,
}

/// Why a job stopped without a final status of its own.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// Nothing was started: the meeting can't run this job now (or can't be read).
    #[error(transparent)]
    NotStarted(LibError),
    /// The job ran, but its sidecar could not be updated.
    #[error(transparent)]
    Storage(LibError),
}

/// `f` under the write lock (poison-tolerant).
pub(crate) fn locked<T>(lock: &Mutex<()>, f: impl FnOnce() -> T) -> T {
    let _g = lock.lock().unwrap_or_else(PoisonError::into_inner);
    f()
}

/// A pending segment: its index, file name and measured duration.
struct Pending {
    index: usize,
    file: String,
    duration_ms: Option<u64>,
    flag: Option<SegmentFlag>,
}

/// What to do with a pending segment.
enum Audio {
    Send(Vec<u8>),
    /// Not sent: store this text, and this flag (ruling 2026-10-08, EC6/EC7).
    Skip(&'static str, SegmentFlag),
}

/// F5-FR1..FR3: send every segment whose `text` is still `null`, in order, storing each text as
/// it arrives. Returns the phase the meeting ends in (`transcribed`, or failed(transcribing)).
pub async fn transcribe(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    plan: &JobPlan,
    cancel: &Cancel,
) -> Result<Phase, JobError> {
    let Some((key, model)) = &plan.transcribe else {
        return Err(JobError::NotStarted(LibError::Invalid(
            "no transcribe provider".into(),
        )));
    };
    let (dir, pending, tid, no_audio) = locked(lock, || {
        lib.update(id, |sc, dir| {
            let p = status::next(library::phase(sc), Event::StartTranscribe)?;
            library::set_phase(sc, p);
            sc.error = None;
            sc.template_id
                .get_or_insert_with(|| plan.default_template.clone());
            sc.providers.transcribe = Some(ProviderRun {
                provider: key.provider,
                model: model.clone(),
                extra: Default::default(),
            });
            let none = sc.segments.is_empty();
            Ok((dir.to_path_buf(), pending(sc), sc.template_id.clone(), none))
        })
    })
    .map_err(JobError::NotStarted)?;
    let fail = |msg: String| finish(lib, lock, id, Event::TranscribeFailed, Some(msg));
    if no_audio {
        return fail(NO_AUDIO.into());
    }
    let vocabulary = vocabulary(plan, tid.as_deref().unwrap_or_default());
    let mut model = model.clone();
    let mut fallback = plan.transcribe_fallback.clone().filter(|f| *f != model);
    for seg in pending {
        if cancel.is_cancelled() {
            return fail("Cancelled".into());
        }
        let bytes = match read_segment(&dir, &seg, key) {
            Ok(Audio::Send(b)) => b,
            Ok(Audio::Skip(text, flag)) => {
                store(lib, lock, id, &seg, text.to_owned(), Some(flag))?;
                continue;
            }
            Err(msg) => return fail(msg),
        };
        let budget = plan.budgets.transcribe(seg.duration_ms);
        let call = |m: String| {
            let (bytes, vocabulary) = (&bytes, &vocabulary);
            let file = seg.file.clone();
            async move {
                let f = || {
                    let audio = AudioIn {
                        bytes: bytes.clone(),
                        file_name: file.clone(),
                        prompt: vocabulary.clone(),
                        duration_ms: seg.duration_ms,
                    };
                    providers::transcribe(&plan.http, &plan.endpoints, key, &m, audio)
                };
                jobs::with_retry(cancel, budget, f).await
            }
        };
        let mut r = call(model.clone()).await;
        // D1: a model the account can't use is retried once, for good, with the fallback.
        if let Err(e) = &r
            && e.kind == CallFail::ModelRejected
            && let Some(fb) = fallback.take()
        {
            model = fb;
            ran_with(lib, lock, id, key, &model)?;
            r = call(model.clone()).await;
        }
        let text = match r {
            Ok(t) => t,
            Err(e) => return fail(e.detail),
        };
        store(lib, lock, id, &seg, text, None)?;
    }
    let p = finish(lib, lock, id, Event::AllSegmentsText, None)?;
    // S4: the transcript file exists as soon as there is a transcript (summary empty); a
    // summary re-renders it. A failure here is reported by the summary's own write.
    let _ = transcript::rewrite(lib, lock, id, &plan.templates, true);
    if plan.summarise.is_none() {
        // Transcribed; Summarise again is offered once the key is there (F7-FR2).
        let problem = plan.summarise_problem.clone();
        locked(lock, || {
            lib.update(id, |sc, _| {
                sc.error = problem;
                Ok(())
            })
        })
        .map_err(JobError::Storage)?;
        return Ok(p);
    }
    summarise(lib, lock, id, plan, cancel).await
}

/// Record that transcription now runs with `model`.
fn ran_with(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    key: &ProviderKey,
    model: &str,
) -> Result<(), JobError> {
    locked(lock, || {
        lib.update(id, |sc, _| {
            sc.providers.transcribe = Some(ProviderRun {
                provider: key.provider,
                model: model.to_owned(),
                extra: Default::default(),
            });
            Ok(())
        })
    })
    .map_err(JobError::Storage)
}

/// Store `text` (and `flag`) on the segment. The sidecar is re-read: only the segment the job
/// took is changed, and only if it is still the same file.
fn store(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    seg: &Pending,
    text: String,
    flag: Option<SegmentFlag>,
) -> Result<(), JobError> {
    locked(lock, || {
        lib.update(id, |sc, _| {
            if let Some(s) = sc
                .segments
                .get_mut(seg.index)
                .filter(|s| s.file == seg.file)
            {
                s.text = Some(text);
                if flag.is_some() {
                    s.flag = flag;
                    s.duration_ms.get_or_insert(0);
                }
            }
            Ok(())
        })
    })
    .map_err(JobError::Storage)
}

/// F5-FR1: the meeting's template vocabulary as the transcription hint ("Priya, Supernote");
/// General's when the template is gone, empty when there is none.
fn vocabulary(plan: &JobPlan, template_id: &str) -> String {
    let t = plan.templates.iter().find(|t| t.id == template_id);
    let t = t.or_else(|| {
        plan.templates
            .iter()
            .find(|t| t.id == crate::templates::DEFAULT_ID)
    });
    t.map(|t| t.vocabulary.join(", ")).unwrap_or_default()
}

fn pending(sc: &Sidecar) -> Vec<Pending> {
    sc.segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.text.is_none())
        .map(|(index, s)| Pending {
            index,
            file: s.file.clone(),
            duration_ms: s.duration_ms,
            flag: s.flag,
        })
        .collect()
}

/// The segment's bytes; or, for one with no audio to send, the text it reads as: a missing
/// file is "(audio missing)", an empty one "(no speech detected)" (a provider rejects a 0-byte
/// file as corrupt, so it is never sent). Else the user-facing reason it can't be sent.
fn read_segment(dir: &Path, seg: &Pending, key: &ProviderKey) -> Result<Audio, String> {
    let n = seg.index + 1;
    let path: PathBuf = dir.join(&seg.file);
    let missing = Audio::Skip(AUDIO_MISSING, SegmentFlag::Missing);
    let empty = Audio::Skip(providers::NO_SPEECH, SegmentFlag::Empty);
    let meta = match (seg.flag, std::fs::metadata(&path)) {
        (Some(SegmentFlag::Missing), _) => return Ok(missing),
        (Some(SegmentFlag::Empty), _) => return Ok(empty),
        (None, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(missing),
        (None, Err(e)) => return Err(format!("Segment {n} can't be read ({:?})", e.kind())),
        (None, Ok(m)) => m,
    };
    if meta.is_file() && meta.len() == 0 {
        return Ok(empty);
    }
    if meta.len() > MAX_AUDIO_BYTES {
        let name = providers::provider_name(key.provider);
        return Err(format!("Segment too large for {name}"));
    }
    std::fs::read(&path)
        .map(Audio::Send)
        .map_err(|e| format!("Segment {n} can't be read ({:?})", e.kind()))
}

/// Apply `event` (and `error`) in one write; returns the phase reached.
pub(crate) fn finish(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    event: Event,
    error: Option<String>,
) -> Result<Phase, JobError> {
    locked(lock, || apply(lib, id, event, error)).map_err(JobError::Storage)
}

/// `finish` without taking the lock (the caller holds it).
pub(crate) fn apply(
    lib: &Library<'_>,
    id: &str,
    event: Event,
    error: Option<String>,
) -> Result<Phase, LibError> {
    lib.update(id, |sc, _| {
        let p = status::next(library::phase(sc), event)?;
        library::set_phase(sc, p);
        sc.error = error;
        Ok(p)
    })
}

/// F5-AC10: `h:mm:ss` from milliseconds (1 800 400 → `0:30:00`); hours don't wrap at 60 min.
pub fn marker(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// F5-FR2: each segment as `(marker, text)`, in segment order. A segment with no text yet, or
/// empty text, reads as `(no speech detected)` (EC6).
pub fn paragraphs(sc: &Sidecar) -> Vec<(String, String)> {
    sc.segments
        .iter()
        .map(|s| {
            let text = match s.text.as_deref().map(str::trim) {
                None | Some("") => providers::NO_SPEECH.to_owned(),
                Some(t) => t.to_owned(),
            };
            (marker(s.start_offset_ms), text)
        })
        .collect()
}

/// The transcript as one text: `[h:mm:ss] text` paragraphs separated by a blank line.
pub fn joined(sc: &Sidecar) -> String {
    paragraphs(sc)
        .iter()
        .map(|(m, t)| format!("[{m}] {t}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        let got = [0, 900_000, 1_800_400, 3_600_000, 36_000_999].map(marker);
        assert_eq!(
            got,
            ["0:00:00", "0:15:00", "0:30:00", "1:00:00", "10:00:00"]
        );
    }
}
