//! F15-FR5 / FR7: transcribing one near-live chunk. The lane (`App::live_drain_json`) sends the
//! chunks one at a time, in index order, with the provider and model fixed at Record, through
//! the same `providers::transcribe` call and error mapping as F5; each success stores the text
//! and its usage record (F16-FR2) in one sidecar write. A failure leaves the chunk `pending`
//! and says what the lane does next: back off and retry (429, 5xx, no connection, timeout), or
//! pause with a reason (a rejected key or model, anything else that would fail again). The lane
//! never watches the network (F15-FR7) and never touches the recording (INV10).
use super::{JobError, Spent, locked};
use crate::jobs::{self, Budgets, Cancel};
use crate::keyfile::ProviderKey;
use crate::library::{self, Library};
use crate::providers::{self, AudioIn, CallError, CallFail, Endpoints, Http};
use crate::rates::RateTable;
use crate::sidecar::ChunkState;
use crate::spend::{Kind, UsageEntry};
use std::path::Path;
use std::sync::Mutex;

/// A chunk with more speech than this that comes back empty is sent once more (F15-FR5,
/// F5-FR13's rule scaled to 30 s).
pub const SPEECH_FOR_RETRY_MS: u64 = 5_000;

/// The chunk's `lastError` while its one empty-reply retry is pending.
pub const EMPTY_ONCE: &str = "No text came back for audio with speech; trying once more";

/// The lane's wait before trying a failed chunk again: 5 s, 15 s, 60 s, then every 60 s while
/// recording. `attempts` counts the failures so far (1 after the first).
pub fn backoff_ms(attempts: u32) -> u64 {
    match attempts {
        0 | 1 => 5_000,
        2 => 15_000,
        _ => 60_000,
    }
}

/// What one chunk is sent with.
pub struct ChunkCall<'a> {
    pub http: &'a Http,
    pub endpoints: &'a Endpoints,
    pub key: &'a ProviderKey,
    pub model: &'a str,
    pub budgets: Budgets,
    pub rates: &'a RateTable,
    /// The template's vocabulary hint (F5-FR1).
    pub vocabulary: String,
}

/// How one attempt ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The text is stored (or the chunk was no longer pending).
    Stored,
    /// Send it again at once: the one retry of an empty reply for audio with speech.
    Again,
    /// Try it again after this many ms (the back-off).
    Retry(u64),
    /// Stop the lane: the reason, as shown once on the meeting page.
    Pause(String),
    /// The chunk's file is gone: it can only be re-cut from its segment after Stop (F15-FR9).
    NoFile,
    Cancelled,
}

/// The failures the lane retries on its own schedule; any other pauses it.
pub fn retried(kind: CallFail) -> bool {
    matches!(
        kind,
        CallFail::Retryable | CallFail::NoConnection | CallFail::TimedOut
    )
}

/// The reason a pause shows ("Transcription paused: OpenAI rejected the key (401)").
pub fn paused_reason(detail: &str) -> String {
    format!("Transcription paused: {detail}")
}

/// One attempt at chunk `index` of `id`, whose file is under `root` (F15-FR5). The sidecar is
/// re-read under the lock for every change, so an edit made meanwhile survives.
pub async fn attempt(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    root: &Path,
    call: &ChunkCall<'_>,
    index: u32,
    cancel: &Cancel,
) -> Result<Outcome, JobError> {
    let (_, sc) = locked(lock, || lib.load_for_write(id)).map_err(JobError::Storage)?;
    let Some(c) = sc
        .chunks
        .iter()
        .find(|c| c.index == index && c.state == ChunkState::Pending)
        .cloned()
    else {
        return Ok(Outcome::Stored);
    };
    let path = library::chunk_path(root, id, index);
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(Outcome::NoFile);
    };
    let audio = AudioIn {
        bytes,
        file_name: library::chunk_file_name(index),
        prompt: call.vocabulary.clone(),
        duration_ms: Some(c.duration_ms),
        empty_ok: true,
    };
    let spent = Spent::new(lib.tz, call.rates);
    let fut = providers::transcribe(call.http, call.endpoints, call.key, call.model, audio);
    let r = jobs::race(cancel, call.budgets.transcribe(Some(c.duration_ms)), fut).await;
    spent.note_chunk(Kind::Transcribe, index, &r);
    let speech = c.speech_ms.is_none_or(|s| s > SPEECH_FOR_RETRY_MS);
    let empty_once = c.last_error.as_deref() == Some(EMPTY_ONCE);
    let (state, text, error, outcome) = match r {
        Ok(rep) if rep.text == providers::NO_SPEECH && speech && !empty_once => (
            ChunkState::Pending,
            None,
            Some(EMPTY_ONCE.to_owned()),
            Outcome::Again,
        ),
        Ok(rep) => (ChunkState::Done, Some(rep.text), None, Outcome::Stored),
        Err(e) => failed(&e, c.attempts + 1),
    };
    let recs = spent.take();
    let stored = locked(lock, || {
        lib.update(id, |sc, _| {
            sc.usage.extend(recs.iter().cloned().map(UsageEntry::from));
            if let Some(c) = sc
                .chunks
                .iter_mut()
                .find(|c| c.index == index && c.state == ChunkState::Pending)
            {
                c.state = state;
                c.text = text;
                c.attempts += 1;
                c.last_error = error;
            }
            Ok(())
        })
    });
    if let Err(e) = stored {
        spent.put_back(recs);
        return Err(JobError::Storage(e));
    }
    Ok(outcome)
}

/// A failed attempt (the chunk's `attempts`-th): it stays pending with the reason; the lane
/// retries after the back-off or pauses.
fn failed(e: &CallError, attempts: u32) -> (ChunkState, Option<String>, Option<String>, Outcome) {
    let outcome = match e.kind {
        CallFail::Cancelled => Outcome::Cancelled,
        k if retried(k) => Outcome::Retry(backoff_ms(attempts)),
        _ => Outcome::Pause(paused_reason(&e.detail)),
    };
    (ChunkState::Pending, None, Some(e.detail.clone()), outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_is_5_15_then_60_s() {
        let waits: Vec<u64> = (1..=5).map(backoff_ms).collect();
        assert_eq!(waits, [5_000, 15_000, 60_000, 60_000, 60_000]);
        assert_eq!(backoff_ms(0), 5_000);
    }

    #[test]
    fn which_failures_are_retried() {
        for k in [
            CallFail::Retryable,
            CallFail::NoConnection,
            CallFail::TimedOut,
        ] {
            assert!(retried(k));
        }
        for k in [
            CallFail::KeyRejected,
            CallFail::ModelRejected,
            CallFail::TooLarge,
            CallFail::Http,
            CallFail::BadResponse,
            CallFail::Cancelled,
        ] {
            assert!(!retried(k));
        }
        let (_, _, err, o) = failed(
            &CallError::new(CallFail::KeyRejected, "OpenAI rejected the key (401)"),
            1,
        );
        assert_eq!(
            o,
            Outcome::Pause("Transcription paused: OpenAI rejected the key (401)".into())
        );
        assert_eq!(err.as_deref(), Some("OpenAI rejected the key (401)"));
        assert_eq!(
            failed(&CallError::new(CallFail::Cancelled, "Cancelled"), 1).3,
            Outcome::Cancelled
        );
        assert_eq!(
            failed(&CallError::new(CallFail::TimedOut, "Timed out"), 2).3,
            Outcome::Retry(15_000)
        );
    }
}
