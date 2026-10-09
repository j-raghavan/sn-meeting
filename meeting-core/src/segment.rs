//! F2-FR5: the rollover plan. Gemini's 20 MB inline request is the strictest cap: rig sends a
//! segment inline as base64, so base64(raw) plus 1 MB of prompt and JSON must stay under 19 MB.
use serde::Serialize;

pub const MB: u64 = 1_000_000;
/// base64(raw) + 1 MB of prompt/JSON must stay under this.
pub const REQUEST_BUDGET: u64 = 19 * MB;
pub const MAX_SEGMENT_MS: u64 = 15 * 60 * 1000;
/// Debug override bounds (F2-AC3 uses 60 s).
pub const MIN_DEBUG_SEGMENT_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentPlan {
    /// `MediaRecorder.setMaxDuration`.
    pub max_duration_ms: u64,
    /// `MediaRecorder.setMaxFileSize`: the largest raw size whose base64 + 1 MB fits.
    pub max_file_bytes: u64,
}

/// Largest raw byte count with `4 * ceil(raw / 3) + 1 MB < 19 MB`.
pub const fn max_raw_bytes() -> u64 {
    ((REQUEST_BUDGET - MB - 1) / 4) * 3
}

/// `None` for a zero bitrate. `debug_ms` (Settings) can only shorten the plan.
pub fn plan(bitrate_bps: u32, debug_ms: Option<u64>) -> Option<SegmentPlan> {
    if bitrate_bps == 0 {
        return None;
    }
    let raw = max_raw_bytes();
    // raw bytes = bitrate/8 * seconds  =>  ms = raw * 8000 / bitrate
    let by_size = raw * 8000 / u64::from(bitrate_bps);
    let mut ms = by_size.min(MAX_SEGMENT_MS);
    if let Some(d) = debug_ms {
        ms = ms.min(d.max(MIN_DEBUG_SEGMENT_MS));
    }
    Some(SegmentPlan {
        max_duration_ms: ms,
        max_file_bytes: raw,
    })
}
