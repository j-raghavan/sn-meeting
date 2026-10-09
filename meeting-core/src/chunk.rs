//! F15: near-live chunks. This part is F15-FR4's cut planning: the shell supplies the PCM RMS
//! of the chunk being written, per 100 ms block from its first sample, and the core decides
//! where it ends. Between 25 s and 35 s the cut goes in the quietest 300 ms window, with no
//! overlap, when that window is below the pause threshold; with no such pause by 35 s the chunk
//! is cut at 35 s and the next one starts 1 s earlier (overlap, removed when the texts are
//! joined). A chunk that must end sooner (its segment ends, or the recording stops) is cut there,
//! with no overlap. Pure and host-tested: no clock, no files.
use serde::{Deserialize, Serialize};

/// The length a chunk aims for.
pub const TARGET_MS: u64 = 30_000;
/// The earliest a pause may end a chunk.
pub const MIN_MS: u64 = 25_000;
/// The latest: with no pause by then the chunk is cut here, with [`OVERLAP_MS`].
pub const MAX_MS: u64 = 35_000;
/// How much earlier the next chunk starts after a hard cut (INV11: at most 1 s per chunk).
pub const OVERLAP_MS: u64 = 1_000;
/// The block the shell measures RMS over.
pub const BLOCK_MS: u64 = 100;
/// The quietest window is this many blocks (300 ms).
pub const WINDOW_BLOCKS: usize = 3;
/// A window whose RMS (16-bit PCM, full scale 32767) is below this is a pause: about −36 dBFS,
/// well under speech at a desk mic and above a quiet room's floor [Recommendation; tune on the
/// device, the runbook's D14 step].
pub const PAUSE_RMS: f64 = 500.0;
/// The most blocks one plan reads (60 s): more is not a chunk.
pub const MAX_BLOCKS: usize = 600;

/// `nativeChunkPlan`'s `rmsJson`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanIn {
    /// RMS per [`BLOCK_MS`] block from the chunk's first sample, oldest first.
    pub rms: Vec<f64>,
    /// Where the chunk must end at the latest, in ms from its start: its segment's end, or the
    /// stop point. `None` while it may run on.
    #[serde(default)]
    pub limit_ms: Option<u64>,
}

/// `nativeChunkPlan`'s reply.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Where to cut, in ms from the chunk's start; `None`: not yet (fewer than 35 s of blocks).
    pub cut_at_ms: Option<u64>,
    /// How much earlier than the cut the next chunk starts (0 or [`OVERLAP_MS`]).
    pub overlap_ms: u64,
    /// The chunk's audio above the pause threshold, up to the cut (F15-FR5's empty-reply rule).
    pub speech_ms: u64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("rms must be finite and >= 0")]
    BadRms,
    #[error("too many rms blocks")]
    TooMany,
}

/// The RMS of `blocks` together (the root of their mean square).
fn window_rms(blocks: &[f64]) -> f64 {
    let n = blocks.len().max(1) as f64;
    (blocks.iter().map(|r| r * r).sum::<f64>() / n).sqrt()
}

/// Ms of the blocks before `cut_ms` whose RMS is above the pause threshold.
fn speech_before(rms: &[f64], cut_ms: u64) -> u64 {
    let blocks = usize::try_from(cut_ms / BLOCK_MS).unwrap_or(usize::MAX);
    rms.iter().take(blocks).filter(|r| **r >= PAUSE_RMS).count() as u64 * BLOCK_MS
}

/// F15-FR4 / F15-AC1: where the chunk described by `input` ends.
pub fn plan(input: &PlanIn) -> Result<Plan, PlanError> {
    if input.rms.len() > MAX_BLOCKS {
        return Err(PlanError::TooMany);
    }
    if input.rms.iter().any(|r| !r.is_finite() || *r < 0.0) {
        return Err(PlanError::BadRms);
    }
    let rms = &input.rms;
    let at = |cut_ms: u64, overlap_ms: u64| Plan {
        cut_at_ms: Some(cut_ms),
        overlap_ms,
        speech_ms: speech_before(rms, cut_ms),
    };
    // A segment boundary or Stop: end there, gapless (a chunk never spans a segment).
    if let Some(limit) = input.limit_ms.filter(|l| *l <= MAX_MS) {
        return Ok(at(limit, 0));
    }
    let have_ms = rms.len() as u64 * BLOCK_MS;
    if have_ms < MAX_MS {
        return Ok(Plan {
            cut_at_ms: None,
            overlap_ms: 0,
            speech_ms: speech_before(rms, have_ms),
        });
    }
    // Every 300 ms window inside [25 s, 35 s]: the quietest, ties to the one nearest 30 s.
    let first = (MIN_MS / BLOCK_MS) as usize;
    let last = (MAX_MS / BLOCK_MS) as usize - WINDOW_BLOCKS;
    let mid_ms = |w: usize| w as u64 * BLOCK_MS + WINDOW_BLOCKS as u64 * BLOCK_MS / 2;
    let best = (first..=last)
        .map(|w| (w, window_rms(&rms[w..w + WINDOW_BLOCKS])))
        .min_by(|(wa, a), (wb, b)| {
            a.total_cmp(b).then_with(|| {
                mid_ms(*wa)
                    .abs_diff(TARGET_MS)
                    .cmp(&mid_ms(*wb).abs_diff(TARGET_MS))
            })
        });
    match best {
        Some((w, r)) if r < PAUSE_RMS => Ok(at(mid_ms(w), 0)),
        _ => Ok(at(MAX_MS, OVERLAP_MS)),
    }
}

/// `nativeChunkPlan(id, rmsJson)`: the plan as JSON, or `{"error"}`. The id is checked only for
/// its shape; the plan is the same for every meeting.
pub fn plan_json(id: &str, rms_json: &str) -> String {
    if !crate::sidecar::is_valid_id(id) {
        return crate::api::error_json("invalid meeting id");
    }
    let input: PlanIn = match serde_json::from_str(rms_json) {
        Ok(i) => i,
        Err(e) => return crate::api::error_json(&format!("rmsJson: {e}")),
    };
    match plan(&input) {
        Ok(p) => serde_json::json!(p).to_string(),
        Err(e) => crate::api::error_json(&e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `secs` of blocks at `level`.
    fn flat(secs: u64, level: f64) -> Vec<f64> {
        vec![level; (secs * 1000 / BLOCK_MS) as usize]
    }

    fn input(rms: Vec<f64>, limit_ms: Option<u64>) -> PlanIn {
        PlanIn { rms, limit_ms }
    }

    #[test]
    fn a_pause_at_28_4_s_is_where_the_chunk_ends_with_no_overlap() {
        // F15-AC1: speech, with 300 ms of quiet starting at 28.4 s.
        let mut rms = flat(36, 2_000.0);
        for b in &mut rms[284..287] {
            *b = 40.0;
        }
        let p = plan(&input(rms, None)).unwrap();
        let cut = p.cut_at_ms.unwrap();
        assert!((28_400..28_700).contains(&cut), "{cut}");
        assert_eq!(p.overlap_ms, 0);
        // Speech up to the cut, less the one quiet block before it.
        assert_eq!(p.speech_ms, (cut / BLOCK_MS - 1) * BLOCK_MS);
    }

    #[test]
    fn no_pause_by_35_s_cuts_there_and_the_next_chunk_starts_at_34_s() {
        let p = plan(&input(flat(35, 3_000.0), None)).unwrap();
        assert_eq!(p.cut_at_ms, Some(MAX_MS));
        assert_eq!(p.overlap_ms, OVERLAP_MS);
        assert_eq!(MAX_MS - p.overlap_ms, 34_000);
        assert_eq!(p.speech_ms, 35_000);
        // Quieter windows that are still speech don't count as a pause.
        let mut rms = flat(35, 3_000.0);
        rms[300..303].fill(PAUSE_RMS);
        assert_eq!(plan(&input(rms, None)).unwrap().cut_at_ms, Some(MAX_MS));
    }

    #[test]
    fn the_quietest_window_wins_and_ties_go_to_the_one_nearest_30_s() {
        let mut rms = flat(35, 2_000.0);
        rms[260..263].fill(100.0);
        rms[320..323].fill(30.0);
        assert_eq!(plan(&input(rms, None)).unwrap().cut_at_ms, Some(32_150));
        // All silent: the window around 30 s.
        let p = plan(&input(flat(35, 0.0), None)).unwrap();
        assert_eq!(
            p.cut_at_ms.map(|c| c.abs_diff(TARGET_MS) <= 100),
            Some(true)
        );
        assert_eq!((p.overlap_ms, p.speech_ms), (0, 0));
        // A pause before 25 s is not a cut.
        let mut rms = flat(35, 2_000.0);
        rms[100..103].fill(0.0);
        assert_eq!(plan(&input(rms, None)).unwrap().cut_at_ms, Some(MAX_MS));
    }

    #[test]
    fn fewer_than_35_s_is_not_yet() {
        let p = plan(&input(flat(20, 2_000.0), None)).unwrap();
        assert_eq!(
            p,
            Plan {
                cut_at_ms: None,
                overlap_ms: 0,
                speech_ms: 20_000
            }
        );
        let p = plan(&input(Vec::new(), None)).unwrap();
        assert_eq!(p.cut_at_ms, None);
    }

    #[test]
    fn a_segment_end_or_stop_cuts_there_with_no_overlap() {
        let p = plan(&input(flat(12, 2_000.0), Some(11_500))).unwrap();
        assert_eq!(
            p,
            Plan {
                cut_at_ms: Some(11_500),
                overlap_ms: 0,
                speech_ms: 11_500
            }
        );
        // A limit past 35 s is planned as usual.
        let p = plan(&input(flat(35, 2_000.0), Some(40_000))).unwrap();
        assert_eq!(p.cut_at_ms, Some(MAX_MS));
        assert_eq!(p.overlap_ms, OVERLAP_MS);
    }

    #[test]
    fn bad_input_is_refused() {
        assert_eq!(plan(&input(vec![f64::NAN], None)), Err(PlanError::BadRms));
        assert_eq!(plan(&input(vec![-1.0], None)), Err(PlanError::BadRms));
        assert_eq!(
            plan(&input(vec![1.0; MAX_BLOCKS + 1], None)),
            Err(PlanError::TooMany)
        );
    }

    #[test]
    fn the_json_call() {
        let id = "meeting-20261009-100000";
        let v: serde_json::Value =
            serde_json::from_str(&plan_json(id, r#"{"rms":[700,700],"limitMs":200}"#)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"cutAtMs": 200, "overlapMs": 0, "speechMs": 200})
        );
        let v: serde_json::Value = serde_json::from_str(&plan_json(id, r#"{"rms":[]}"#)).unwrap();
        assert_eq!(v["cutAtMs"], serde_json::Value::Null);
        assert!(plan_json("x", "{}").contains("invalid meeting id"));
        assert!(plan_json(id, "nope").contains("rmsJson"));
        assert!(plan_json(id, r#"{"rms":[-2]}"#).contains("finite"));
    }
}
