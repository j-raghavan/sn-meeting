//! F5-FR2 / FR3: the transcribe job, then summarise (`summary.rs`, F5-FR8). Every sidecar change is one short read-modify-write under
//! the core write lock (`Library::update`); no sidecar copy is held across an await, so an edit
//! made while a request is in flight (a to-do ticked, a rename) is never overwritten.
use crate::jobs::{self, Cancel};
use crate::keyfile::ProviderKey;
use crate::library::{self, LibError, Library};
use crate::providers::{self, AudioIn, CallError, CallFail, Endpoints, Http, Replied};
use crate::rates::RateTable;
use crate::sidecar::{ProviderRun, SegmentFlag, Sidecar};
use crate::spend::{Kind, UsageEntry, UsageRecord};
use crate::status::{self, Event, Phase};
use crate::tz::{self, LocalTz};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

pub mod live;
mod notes;
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
    /// The rates in force at job start, pricing each usage record once (F16-FR3).
    pub rates: RateTable,
    /// Where the summarise step gets the note regions to recognise (F19-FR6).
    pub notes: crate::notes_text::NotesSource,
    /// F18-FR12 / OQ25: send a linked meeting's calendar event with the minutes call.
    pub event_block: bool,
    /// F15-FR9 / FR10: where a near-live meeting's chunk files are, and the lock its chunks are
    /// sent under; `None` sends segments only (F5's path).
    pub chunks: Option<ChunkCtx>,
}

/// F15-FR9: what a job needs to send a near-live meeting's chunks.
#[derive(Clone, Debug)]
pub struct ChunkCtx {
    /// `<filesDir>/chunks`.
    pub root: PathBuf,
    /// Held while the chunks are sent: an in-flight near-live chunk finishes first (INV11).
    pub lane: std::sync::Arc<tokio::sync::Mutex<()>>,
}

/// F16-FR2: the usage records of calls whose result is not yet written. Each call that got a
/// reply adds one; the next sidecar write (the result's, or the failure's) takes them all, so a
/// record lands in the same write as what its call produced.
pub(crate) struct Spent<'a> {
    tz: &'a dyn LocalTz,
    rates: &'a RateTable,
    records: Mutex<Vec<UsageRecord>>,
}

impl<'a> Spent<'a> {
    pub(crate) fn new(tz: &'a dyn LocalTz, rates: &'a RateTable) -> Spent<'a> {
        Spent {
            tz,
            rates,
            records: Mutex::default(),
        }
    }

    /// Record what `r`'s reply reported, if a reply arrived (OQ16: a failed request reports
    /// nothing and costs nothing here). `segment` is the segment transcribed.
    pub(crate) fn note(&self, kind: Kind, segment: Option<usize>, r: &Result<Replied, CallError>) {
        self.note_at(kind, segment, None, r);
    }

    /// `note` for near-live chunk `chunk` (F15-FR5).
    pub(crate) fn note_chunk(&self, kind: Kind, chunk: u32, r: &Result<Replied, CallError>) {
        self.note_at(kind, None, Some(chunk), r);
    }

    fn note_at(
        &self,
        kind: Kind,
        segment: Option<usize>,
        chunk: Option<u32>,
        r: &Result<Replied, CallError>,
    ) {
        let m = match r {
            Ok(x) => &x.measured,
            Err(e) => match &e.spent {
                Some(m) => m.as_ref(),
                None => return,
            },
        };
        let now = chrono::Utc::now().timestamp_millis();
        let at = tz::local_at(self.tz, now)
            .map(|t| tz::rfc3339(&t))
            .unwrap_or_default();
        let mut rec = UsageRecord::new(kind, m, at, self.rates);
        rec.segment = segment;
        rec.chunk = chunk;
        self.lock().push(rec);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<UsageRecord>> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every record not yet written, oldest first.
    pub(crate) fn take(&self) -> Vec<UsageRecord> {
        std::mem::take(&mut *self.lock())
    }

    /// Records whose write failed, back in front of any noted since.
    pub(crate) fn put_back(&self, mut recs: Vec<UsageRecord>) {
        let mut g = self.lock();
        recs.append(&mut g);
        *g = recs;
    }
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
    run_transcribe(lib, lock, id, plan, cancel, false).await
}

/// F5-FR13: "Transcribe again" from `transcribed` or `done`: every segment's text is cleared
/// (to-dos, ticks and titles are kept), then it runs as [`transcribe`], summary included.
pub async fn transcribe_again(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    plan: &JobPlan,
    cancel: &Cancel,
) -> Result<Phase, JobError> {
    run_transcribe(lib, lock, id, plan, cancel, true).await
}

async fn run_transcribe(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    plan: &JobPlan,
    cancel: &Cancel,
    again: bool,
) -> Result<Phase, JobError> {
    let Some((key, model)) = &plan.transcribe else {
        return Err(JobError::NotStarted(LibError::Invalid(
            "no transcribe provider".into(),
        )));
    };
    let (dir, pending, tid, no_audio, live) = locked(lock, || {
        lib.update(id, |sc, dir| {
            let event = if again {
                Event::TranscribeAgain
            } else {
                Event::StartTranscribe
            };
            let p = status::next(library::phase(sc), event)?;
            library::set_phase(sc, p);
            if again {
                for s in &mut sc.segments {
                    s.text = None;
                }
            }
            sc.error = None;
            sc.template_id
                .get_or_insert_with(|| plan.default_template.clone());
            sc.providers.transcribe = Some(ProviderRun {
                provider: key.provider,
                model: model.clone(),
                extra: Default::default(),
            });
            let none = sc.segments.is_empty();
            // F15-FR9: a near-live meeting is sent as its chunks, with the model fixed at Record
            // when its provider is still the one chosen (F5-FR10).
            let live = (!sc.chunks.is_empty()).then(|| {
                sc.live
                    .as_ref()
                    .filter(|l| l.provider == key.provider)
                    .and_then(|l| l.model.clone())
            });
            Ok((
                dir.to_path_buf(),
                pending(sc),
                sc.template_id.clone(),
                none,
                live,
            ))
        })
    })
    .map_err(JobError::NotStarted)?;
    let spent = Spent::new(lib.tz, &plan.rates);
    let fail = |msg: String| {
        let event = Event::TranscribeFailed;
        finish_spent(lib, lock, id, event, Some(msg), &spent)
    };
    if no_audio {
        return fail(NO_AUDIO.into());
    }
    let vocabulary = vocabulary(plan, tid.as_deref().unwrap_or_default());
    if let (Some(fixed), Some(cx)) = (live, &plan.chunks) {
        let model = fixed.unwrap_or_else(|| model.clone());
        ran_with(lib, lock, id, key, &model)?;
        let call = live::ChunkCall {
            http: &plan.http,
            endpoints: &plan.endpoints,
            key,
            model: &model,
            budgets: plan.budgets,
            rates: &plan.rates,
            vocabulary,
        };
        if let Some(msg) = live::drain_job(lib, lock, id, cx, &call, cancel).await? {
            return fail(msg);
        }
        return transcribed(lib, lock, id, plan, cancel).await;
    }
    let mut model = model.clone();
    let mut fallback = plan.transcribe_fallback.clone().filter(|f| *f != model);
    for seg in pending {
        if cancel.is_cancelled() {
            return fail("Cancelled".into());
        }
        let bytes = match read_segment(&dir, &seg, key) {
            Ok(Audio::Send(b)) => b,
            Ok(Audio::Skip(text, flag)) => {
                store(lib, lock, id, &seg, text.to_owned(), Some(flag), &spent)?;
                continue;
            }
            Err(msg) => return fail(msg),
        };
        let budget = plan.budgets.transcribe(seg.duration_ms);
        let index = seg.index;
        let call = |m: String| {
            let (bytes, vocabulary, spent) = (&bytes, &vocabulary, &spent);
            let file = seg.file.clone();
            async move {
                let f = || {
                    let audio = AudioIn {
                        bytes: bytes.clone(),
                        file_name: file.clone(),
                        prompt: vocabulary.clone(),
                        duration_ms: seg.duration_ms,
                        empty_ok: false,
                    };
                    let fut = providers::transcribe(&plan.http, &plan.endpoints, key, &m, audio);
                    async move {
                        let r = fut.await;
                        spent.note(Kind::Transcribe, Some(index), &r);
                        r
                    }
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
            Ok(t) => t.text,
            Err(e) => return fail(e.detail),
        };
        store(lib, lock, id, &seg, text, None, &spent)?;
    }
    transcribed(lib, lock, id, plan, cancel).await
}

/// Every segment (or chunk) has its text: `transcribed`, the transcript file, then the summary
/// (F5-FR2/FR11; for a near-live meeting the one minutes call at Stop, D15).
async fn transcribed(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    plan: &JobPlan,
    cancel: &Cancel,
) -> Result<Phase, JobError> {
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

/// Store `text` (and `flag`) on the segment, with the usage records of the calls that produced
/// it (F16-FR2). The sidecar is re-read: only the segment the job took is changed, and only if
/// it is still the same file.
fn store(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    seg: &Pending,
    text: String,
    flag: Option<SegmentFlag>,
    spent: &Spent<'_>,
) -> Result<(), JobError> {
    let recs = spent.take();
    let r = locked(lock, || {
        lib.update(id, |sc, _| {
            sc.usage.extend(recs.iter().cloned().map(UsageEntry::from));
            if let Some(s) = sc
                .segments
                .get_mut(seg.index)
                .filter(|s| s.file == seg.file)
            {
                s.text = Some(text);
                // Sent: any old flag is gone (S10). Skipped: flagged, at 0 ms if unmeasured.
                s.flag = flag;
                if flag.is_some() {
                    s.duration_ms.get_or_insert(0);
                }
            }
            Ok(())
        })
    });
    if r.is_err() {
        spent.put_back(recs);
    }
    r.map_err(JobError::Storage)
}

/// F5-FR1: the meeting's template vocabulary as the transcription hint ("Priya, Supernote");
/// General's when the template is gone, empty when there is none.
fn vocabulary(plan: &JobPlan, template_id: &str) -> String {
    vocabulary_of(&plan.templates, template_id)
}

/// The vocabulary hint of `template_id` among `templates` (General's when it is gone).
pub fn vocabulary_of(templates: &[crate::templates::Template], template_id: &str) -> String {
    let t = templates.iter().find(|t| t.id == template_id);
    let t = t.or_else(|| {
        templates
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
/// file as corrupt, so it is never sent). Else the user-facing reason it can't be sent. The
/// file is checked even when the segment is flagged.
fn read_segment(dir: &Path, seg: &Pending, key: &ProviderKey) -> Result<Audio, String> {
    let n = seg.index + 1;
    let path: PathBuf = dir.join(&seg.file);
    let missing = Audio::Skip(AUDIO_MISSING, SegmentFlag::Missing);
    let empty = Audio::Skip(providers::NO_SPEECH, SegmentFlag::Empty);
    // A flag is re-checked (S10): a file put back since it was flagged is sent, and the flag
    // cleared when its text is stored.
    let meta = match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(missing),
        Err(_) if seg.flag == Some(SegmentFlag::Missing) => return Ok(missing),
        Err(e) => return Err(format!("Segment {n} can't be read ({:?})", e.kind())),
        Ok(m) => m,
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

/// `finish`, writing the usage records `spent` holds in the same write (F16-FR2).
pub(crate) fn finish_spent(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    event: Event,
    error: Option<String>,
    spent: &Spent<'_>,
) -> Result<Phase, JobError> {
    let recs = spent.take();
    locked(lock, || apply_with(lib, id, event, error, recs)).map_err(JobError::Storage)
}

/// `finish` without taking the lock (the caller holds it).
pub(crate) fn apply(
    lib: &Library<'_>,
    id: &str,
    event: Event,
    error: Option<String>,
) -> Result<Phase, LibError> {
    apply_with(lib, id, event, error, Vec::new())
}

fn apply_with(
    lib: &Library<'_>,
    id: &str,
    event: Event,
    error: Option<String>,
    recs: Vec<UsageRecord>,
) -> Result<Phase, LibError> {
    lib.update(id, |sc, _| {
        let p = status::next(library::phase(sc), event)?;
        library::set_phase(sc, p);
        sc.error = error;
        sc.usage.extend(recs.into_iter().map(UsageEntry::from));
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
    // F15-FR6: a near-live meeting's transcript is its chunks', one paragraph each.
    if !sc.chunks.is_empty() {
        return crate::chunk::paragraphs(sc)
            .into_iter()
            .map(|p| (marker(p.at_ms), p.text))
            .collect();
    }
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
