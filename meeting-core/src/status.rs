//! The status machine (spec Data Model) as one pure table. The core is its only writer, and a
//! transition not in the table is refused, leaving the sidecar untouched (INV8).
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Recording,
    Recorded,
    Transcribing,
    Transcribed,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailedAt {
    Transcribing,
    Summarising,
}

/// `status` + `failedAt` folded into one value, so `Failed` always says which step to retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    Recording,
    Recorded,
    Transcribing,
    Transcribed,
    Done,
    Failed(FailedAt),
}

impl Phase {
    pub const ALL: [Phase; 7] = [
        Phase::Recording,
        Phase::Recorded,
        Phase::Transcribing,
        Phase::Transcribed,
        Phase::Done,
        Phase::Failed(FailedAt::Transcribing),
        Phase::Failed(FailedAt::Summarising),
    ];

    /// From the sidecar's two fields. `failed` with a null/stray `failedAt` is read as
    /// `Failed(Transcribing)`: retrying it resumes at the first untranscribed segment, or goes
    /// straight to `transcribed` when every segment has text. A non-failed status drops `failedAt`.
    pub fn from_parts(status: Status, failed_at: Option<FailedAt>) -> Phase {
        match status {
            Status::Recording => Phase::Recording,
            Status::Recorded => Phase::Recorded,
            Status::Transcribing => Phase::Transcribing,
            Status::Transcribed => Phase::Transcribed,
            Status::Done => Phase::Done,
            Status::Failed => Phase::Failed(failed_at.unwrap_or(FailedAt::Transcribing)),
        }
    }

    pub fn parts(self) -> (Status, Option<FailedAt>) {
        match self {
            Phase::Recording => (Status::Recording, None),
            Phase::Recorded => (Status::Recorded, None),
            Phase::Transcribing => (Status::Transcribing, None),
            Phase::Transcribed => (Status::Transcribed, None),
            Phase::Done => (Status::Done, None),
            Phase::Failed(at) => (Status::Failed, Some(at)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// `nativeFinishMeeting` (normal stop or mic lost, F2-FR3).
    Stop,
    /// `recover`: a `recording` sidecar with no live service (F3-FR5).
    RecoverNoService,
    /// `recover`: a `transcribing` sidecar with no job in flight (F3-FR5).
    RecoverNoJob,
    /// `nativeTranscribe` starts (M4).
    StartTranscribe,
    /// Every segment has text (F5-FR2).
    AllSegmentsText,
    /// A segment failed, was missing (EC7) or the job was cancelled.
    TranscribeFailed,
    /// Summarise starts: automatically after transcription, or "Summarise again" (F5-FR8).
    StartSummarise,
    /// TODOs merged and the transcript rendered (F5-FR5).
    SummariseOk,
    SummariseFailed,
}

impl Event {
    pub const ALL: [Event; 9] = [
        Event::Stop,
        Event::RecoverNoService,
        Event::RecoverNoJob,
        Event::StartTranscribe,
        Event::AllSegmentsText,
        Event::TranscribeFailed,
        Event::StartSummarise,
        Event::SummariseOk,
        Event::SummariseFailed,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{event:?} is not allowed while {from:?}")]
pub struct Illegal {
    pub from: Phase,
    pub event: Event,
}

/// The whole machine. Anything not listed is `Illegal` (and the sidecar is left untouched).
pub fn next(from: Phase, event: Event) -> Result<Phase, Illegal> {
    use Event as E;
    use FailedAt::{Summarising, Transcribing};
    use Phase as P;
    let to = match (from, event) {
        (P::Recording, E::Stop | E::RecoverNoService) => P::Recorded,
        (P::Recorded | P::Failed(Transcribing), E::StartTranscribe) => P::Transcribing,
        (P::Transcribing, E::AllSegmentsText) => P::Transcribed,
        (P::Transcribing, E::TranscribeFailed | E::RecoverNoJob) => P::Failed(Transcribing),
        // Summarising has no status of its own: a crash mid-summary leaves `transcribed`/`done`,
        // both of which offer "Summarise again" (INV8).
        (P::Transcribed | P::Failed(Summarising), E::StartSummarise) => P::Transcribed,
        (P::Done, E::StartSummarise) => P::Done,
        (P::Transcribed | P::Done, E::SummariseOk) => P::Done,
        (P::Transcribed | P::Done, E::SummariseFailed) => P::Failed(Summarising),
        _ => return Err(Illegal { from, event }),
    };
    Ok(to)
}

/// What the meeting screen can offer (F7-FR2), with no service or job running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Transcribe,
    SummariseAgain,
}

/// `None` = in progress (a live service or job owns it). Play, rename and delete are always
/// offered and not listed.
pub fn job_actions(p: Phase) -> Option<&'static [Action]> {
    match p {
        Phase::Recording | Phase::Transcribing => None,
        Phase::Recorded | Phase::Failed(FailedAt::Transcribing) => Some(&[Action::Transcribe]),
        Phase::Transcribed | Phase::Done | Phase::Failed(FailedAt::Summarising) => {
            Some(&[Action::SummariseAgain])
        }
    }
}
