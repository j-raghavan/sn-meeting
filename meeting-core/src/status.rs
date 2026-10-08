//! The meeting status as stored in the sidecar (spec Data Model).
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

/// Which step a `failed` meeting retries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailedAt {
    Transcribing,
    Summarising,
}
