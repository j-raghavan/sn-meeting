//! F2-FR5: segment length and size caps for MediaRecorder.
use meeting::segment::{
    MAX_SEGMENT_MS, MB, MIN_DEBUG_SEGMENT_MS, REQUEST_BUDGET, SegmentPlan, max_raw_bytes, plan,
};

fn request_bytes(raw: u64) -> u64 {
    4 * raw.div_ceil(3) + MB
}

#[test]
fn the_raw_cap_is_the_largest_that_fits_the_request() {
    assert_eq!(max_raw_bytes(), 13_499_997);
    assert!(request_bytes(max_raw_bytes()) < REQUEST_BUDGET);
    assert!(request_bytes(max_raw_bytes() + 1) >= REQUEST_BUDGET);
}

#[test]
fn usb_and_bluetooth_are_capped_at_fifteen_minutes() {
    let want = SegmentPlan {
        max_duration_ms: MAX_SEGMENT_MS,
        max_file_bytes: 13_499_997,
    };
    assert_eq!(plan(96_000, None), Some(want));
    assert_eq!(plan(32_000, None), Some(want));
    // 15 min at 96 kbps is 10.8 MB raw, about 14.4 MB as base64: inside the budget.
    let raw = 96_000 / 8 * 900;
    assert_eq!(raw, 10_800_000);
    assert!(request_bytes(raw) < REQUEST_BUDGET);
}

#[test]
fn a_high_bitrate_is_limited_by_size() {
    let p = plan(256_000, None).unwrap();
    assert_eq!(p.max_duration_ms, 13_499_997 * 8000 / 256_000);
    assert!(p.max_duration_ms < MAX_SEGMENT_MS);
    assert!(p.max_duration_ms * 256_000 / 8000 <= max_raw_bytes());
    assert!(plan(u32::MAX, None).unwrap().max_duration_ms < 30);
}

#[test]
fn the_debug_override_only_shortens_and_has_a_floor() {
    assert_eq!(plan(96_000, Some(60_000)).unwrap().max_duration_ms, 60_000);
    assert_eq!(
        plan(96_000, Some(1)).unwrap().max_duration_ms,
        MIN_DEBUG_SEGMENT_MS
    );
    assert_eq!(plan(96_000, Some(0)).unwrap().max_duration_ms, 10_000);
    assert_eq!(
        plan(96_000, Some(60 * 60 * 1000)).unwrap().max_duration_ms,
        MAX_SEGMENT_MS
    );
}

#[test]
fn a_zero_bitrate_has_no_plan() {
    assert_eq!(plan(0, None), None);
    let json = serde_json::to_value(plan(96_000, None).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"maxDurationMs": 900_000, "maxFileBytes": 13_499_997})
    );
}
