//! F3-FR2: add, close and finish update the sidecar atomically and refuse what doesn't fit.
mod libcommon;

use libcommon::*;
use meeting::sidecar::SegmentFlag;
use meeting::status::Status;
use serde_json::json;
use std::fs;

fn seg(dir: &std::path::Path, n: u32) -> String {
    dir.join(format!("{ID0}-{n:02}.m4a"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_two_segment_meeting_from_begin_to_finish() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let dir = e.export();
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, 900_000)),
        json!({"ok": true})
    );
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&dir, 2), 900_120)),
        json!({"index": 1, "path": seg(&dir, 2)})
    );
    assert_eq!(
        parse(&app.close_segment_json(ID0, 1, 300_000)),
        json!({"ok": true})
    );
    assert_eq!(
        parse(&app.finish_json(ID0, 1_200_200, "")),
        json!({"ok": true})
    );
    let sc = read_sidecar(&dir, ID0);
    assert_eq!(
        (sc.status, sc.duration_ms, sc.note.clone()),
        (Status::Recorded, 1_200_200, None)
    );
    let segs = serde_json::to_value(&sc.segments).unwrap();
    assert_eq!(
        segs[1],
        json!({"file": format!("{ID0}-02.m4a"), "startOffsetMs": 900_120, "durationMs": 300_000, "text": null})
    );
    assert_eq!(app.liveness().recording, None);
    assert!(
        begin(&app, T0 + 3_600_000)["id"].is_string(),
        "a new recording may start"
    );
}

#[test]
fn add_segment_accepts_only_the_next_path_in_the_meetings_folder() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let dir = e.export();
    for bad in [
        seg(&dir, 1),
        seg(&dir, 3),
        format!("{}/../{ID0}-02.m4a", dir.display()),
        format!("{ID0}-02.m4a"),
        e.root
            .path()
            .join(format!("Document/{ID0}-02.m4a"))
            .to_string_lossy()
            .into_owned(),
    ] {
        let r = parse(&app.add_segment_json(ID0, &bad, 1000));
        assert_eq!(
            r,
            json!({"error": format!("next segment must be {}", seg(&dir, 2))}),
            "{bad}"
        );
    }
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&dir, 2), -1)),
        json!({"error": "startOffsetMs must be >= 0"})
    );
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&dir, 2), 0)),
        json!({"error": "startOffsetMs must be after the previous segment's"})
    );
    app.add_segment_json(ID0, &seg(&dir, 2), 1000);
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&dir, 3), 1000))["error"],
        "startOffsetMs must be after the previous segment's"
    );
    app.finish_json(ID0, 2000, "");
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&dir, 3), 3000)),
        json!({"error": "meeting is not recording"})
    );
}

#[test]
fn close_segment_rules() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let dir = e.export();
    assert_eq!(
        parse(&app.close_segment_json(ID0, -1, 10)),
        json!({"error": "segIndex must be >= 0"})
    );
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, -5)),
        json!({"error": "durationMs must be >= 0"})
    );
    assert_eq!(
        parse(&app.close_segment_json(ID0, 1, 10)),
        json!({"error": "segment 1 is not open"})
    );
    // 0 ms: closed and flagged empty.
    app.close_segment_json(ID0, 0, 0);
    assert_eq!(
        read_sidecar(&dir, ID0).segments[0].flag,
        Some(SegmentFlag::Empty)
    );
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, 10)),
        json!({"error": "segment 0 is not open"})
    );
    // Still allowed once recorded, for recovery's measurements.
    app.add_segment_json(ID0, &seg(&dir, 2), 5);
    app.finish_json(ID0, 100, "");
    assert_eq!(
        parse(&app.close_segment_json(ID0, 1, 95)),
        json!({"ok": true})
    );
    assert_eq!(read_sidecar(&dir, ID0).segments[1].duration_ms, Some(95));
    // Not once the meeting has moved on.
    let mut sc = read_sidecar(&dir, ID0);
    sc.status = Status::Transcribing;
    sc.segments[1].duration_ms = None;
    meeting::sidecar::write(&dir, &sc).unwrap();
    assert_eq!(
        parse(&app.close_segment_json(ID0, 1, 95)),
        json!({"error": "meeting is past recording"})
    );
}

#[test]
fn finish_notes_why_and_happens_once() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        parse(&app.finish_json(ID0, -1, "")),
        json!({"error": "durationMs must be >= 0"})
    );
    assert_eq!(
        app.liveness().recording,
        None,
        "the recording is over even if finish failed"
    );
    app.finish_json(ID0, 61_000, "  mic\ndisconnected  ");
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!(
        (sc.status, sc.note.as_deref()),
        (Status::Recorded, Some("mic disconnected"))
    );
    assert_eq!(
        parse(&app.finish_json(ID0, 61_000, "")),
        json!({"error": "Stop is not allowed while Recorded"})
    );
    let e2 = Env::new();
    let app2 = e2.app();
    begin(&app2, T0);
    app2.finish_json(ID0, 1, &"n".repeat(500));
    assert_eq!(
        read_sidecar(&e2.export(), ID0)
            .note
            .unwrap()
            .chars()
            .count(),
        200
    );
}

#[test]
fn a_meeting_keeps_its_folder_when_the_save_folder_changes() {
    // F10-AC3, mid-recording: the open meeting stays where it began.
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let r = parse(&app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#));
    assert_eq!(r["settings"]["saveFolder"], "Document/Meetings");
    let new_dir = e.root.path().join("Document/Meetings");
    assert!(parse(&app.add_segment_json(ID0, &seg(&new_dir, 2), 1000))["error"].is_string());
    assert_eq!(
        parse(&app.add_segment_json(ID0, &seg(&e.export(), 2), 1000))["index"],
        1
    );
    assert_eq!(parse(&app.finish_json(ID0, 2000, "")), json!({"ok": true}));
    assert_eq!(read_sidecar(&e.export(), ID0).segments.len(), 2);
    let next = begin(&app, T0 + 60_000);
    assert!(
        next["firstSegmentPath"]
            .as_str()
            .unwrap()
            .starts_with(new_dir.to_str().unwrap())
    );
}

#[test]
fn unknown_bad_and_unreadable_meetings() {
    let e = Env::new();
    let app = e.app();
    assert_eq!(
        parse(&app.finish_json("meeting-20261007-000000", 1, "")),
        json!({"error": "meeting not found"})
    );
    assert_eq!(
        parse(&app.close_segment_json("../etc", 0, 1)),
        json!({"error": "invalid meeting id"})
    );
    begin(&app, T0);
    let path = e.export().join(format!("{ID0}.json"));
    fs::write(&path, "{broken").unwrap();
    let r = parse(&app.finish_json(ID0, 1, ""));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("sidecar is not valid JSON"),
        "{r}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{broken",
        "a bad sidecar is never overwritten"
    );
    // Audio with no sidecar can't be recorded into.
    fs::write(e.export().join("meeting-20261007-111111.m4a"), b"x").unwrap();
    assert_eq!(
        parse(&app.close_segment_json("meeting-20261007-111111", 0, 1)),
        json!({"error": "meeting not found"})
    );
}
