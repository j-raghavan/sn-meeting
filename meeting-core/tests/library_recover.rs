//! F3-FR5 / INV8: crash recovery on launch.
mod libcommon;

use libcommon::*;
use meeting::sidecar::SegmentFlag;
use meeting::status::{FailedAt, Status};
use serde_json::{Value, json};
use std::fs;

/// A meeting left `recording` by a dead process: begun by one app, recovered by a fresh one.
fn crashed_while_recording(e: &Env) {
    let app = e.app();
    begin(&app, T0);
    fs::write(e.export().join(format!("{ID0}-01.m4a")), b"aac frames").unwrap();
}

fn recover(app: &meeting::app::App, alive: bool) -> Value {
    parse(&app.recover_json(alive))
}

#[test]
fn recording_without_a_service_becomes_recorded_and_its_open_segment_is_measured() {
    // F3-AC2, F3-AC6.
    let e = Env::new();
    crashed_while_recording(&e);
    let app = e.app();
    let r = recover(&app, false);
    let path = e.export().join(format!("{ID0}-01.m4a"));
    assert_eq!(
        r,
        json!({"recovered": [ID0], "measure": [{"id": ID0, "index": 0, "path": path}], "flagged": []})
    );
    assert_eq!(parse(&app.list_recent_json(1))[0]["status"], "recorded");
    // The shell measures the file and closes the segment.
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, 41_000)),
        json!({"ok": true})
    );
    assert_eq!(
        read_sidecar(&e.export(), ID0).segments[0].duration_ms,
        Some(41_000)
    );
    assert_eq!(
        recover(&app, false),
        json!({"recovered": [], "measure": [], "flagged": []})
    );
}

#[test]
fn missing_and_empty_segments_are_flagged_never_deleted() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let dir = e.export();
    app.close_segment_json(ID0, 0, 900_000);
    app.add_segment_json(
        ID0,
        dir.join(format!("{ID0}-02.m4a")).to_str().unwrap(),
        900_100,
    );
    app.add_segment_json(
        ID0,
        dir.join(format!("{ID0}-03.m4a")).to_str().unwrap(),
        1_800_200,
    );
    // -01 is gone (deleted on the desktop), -02 is empty, -03 was never created.
    fs::write(dir.join(format!("{ID0}-02.m4a")), b"").unwrap();
    let fresh = e.app();
    let r = recover(&fresh, false);
    assert_eq!(r["recovered"], json!([ID0]));
    assert_eq!(
        r["flagged"],
        json!([{"id": ID0, "index": 0, "flag": "missing"}, {"id": ID0, "index": 1, "flag": "empty"},
               {"id": ID0, "index": 2, "flag": "missing"}])
    );
    assert_eq!(r["measure"], json!([]));
    let sc = read_sidecar(&dir, ID0);
    let got: Vec<(Option<u64>, Option<SegmentFlag>)> = sc
        .segments
        .iter()
        .map(|s| (s.duration_ms, s.flag))
        .collect();
    assert_eq!(
        got,
        [
            (Some(900_000), Some(SegmentFlag::Missing)),
            (Some(0), Some(SegmentFlag::Empty)),
            (Some(0), Some(SegmentFlag::Missing))
        ]
    );
    assert!(
        dir.join(format!("{ID0}-02.m4a")).exists(),
        "an empty file is kept"
    );
    assert_eq!(
        recover(&fresh, false),
        json!({"recovered": [], "measure": [], "flagged": []}),
        "idempotent"
    );
}

#[test]
fn transcribing_without_a_job_fails_and_keeps_its_text() {
    // F3-AC4.
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let dir = e.export();
    app.close_segment_json(ID0, 0, 900_000);
    app.add_segment_json(
        ID0,
        dir.join(format!("{ID0}-02.m4a")).to_str().unwrap(),
        900_100,
    );
    app.close_segment_json(ID0, 1, 60_000);
    app.finish_json(ID0, 960_100, "");
    let mut sc = read_sidecar(&dir, ID0);
    sc.status = Status::Transcribing;
    sc.segments[0].text = Some("first part".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    let r = recover(&e.app(), true);
    assert_eq!(r["recovered"], json!([ID0]));
    let sc = read_sidecar(&dir, ID0);
    assert_eq!(
        (sc.status, sc.failed_at),
        (Status::Failed, Some(FailedAt::Transcribing))
    );
    assert_eq!(sc.error.as_deref(), Some("Transcription was interrupted"));
    let texts: Vec<Option<&str>> = sc.segments.iter().map(|s| s.text.as_deref()).collect();
    assert_eq!(
        texts,
        [Some("first part"), None],
        "a retry resumes at segment 2"
    );
}

#[test]
fn the_live_recording_and_running_jobs_are_left_alone() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        recover(&app, true),
        json!({"recovered": [], "measure": [], "flagged": []})
    );
    assert_eq!(read_sidecar(&e.export(), ID0).status, Status::Recording);
    assert_eq!(app.liveness().recording.as_deref(), Some(ID0));
    // A transcribing meeting with its job in flight stays transcribing.
    let id2 = "meeting-20261007-211706b";
    meeting::sidecar::write(
        &e.export(),
        &meeting::sidecar::Sidecar {
            started_at: "2026-10-07T21:17:06-07:00".into(),
            status: Status::Transcribing,
            ..meeting::sidecar::blank(id2)
        },
    )
    .unwrap();
    app.set_job(id2, true);
    assert_eq!(recover(&app, true)["recovered"], json!([]));
    // Once the service is reported dead, the recording is recovered too.
    assert_eq!(recover(&app, false)["recovered"], json!([ID0]));
    assert_eq!(app.liveness().recording, None);
    app.set_job(id2, false);
    assert_eq!(recover(&app, false)["recovered"], json!([id2]));
}

#[test]
fn other_meetings_are_untouched() {
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    fs::write(dir.join("meeting-20261007-211359.m4a"), b"spike").unwrap();
    fs::write(dir.join("meeting-20261006-090000.json"), "{broken").unwrap();
    let app = e.app();
    begin(&app, T0);
    app.close_segment_json(ID0, 0, 1000);
    fs::write(dir.join(format!("{ID0}-01.m4a")), b"aac").unwrap();
    app.finish_json(ID0, 1000, "");
    let before = fs::read(dir.join(format!("{ID0}.json"))).unwrap();
    // Legacy audio is only listed for measuring (D3); recovery itself writes nothing.
    let spike = dir.join("meeting-20261007-211359.m4a");
    assert_eq!(
        recover(&app, false),
        json!({"recovered": [], "flagged": [], "measure": [
            {"id": "meeting-20261007-211359", "index": 0, "path": spike}]})
    );
    assert_eq!(fs::read(dir.join(format!("{ID0}.json"))).unwrap(), before);
    assert!(!dir.join("meeting-20261007-211359.json").exists());
    assert_eq!(
        fs::read_to_string(dir.join("meeting-20261006-090000.json")).unwrap(),
        "{broken"
    );
}

#[cfg(unix)]
#[test]
fn a_meeting_that_cannot_be_written_is_retried_next_time() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    crashed_while_recording(&e);
    let dir = e.export();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let r = recover(&e.app(), false);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(r["recovered"], json!([]));
    assert_eq!(recover(&e.app(), false)["recovered"], json!([ID0]));
}

#[test]
fn failed_without_failed_at_is_normalised() {
    // INV8: a hand-edited sidecar still offers an action.
    let e = Env::new();
    let app = e.app();
    let a = begin(&app, T0)["id"].as_str().unwrap().to_owned();
    app.finish_json(&a, 1000, "");
    let b = begin(&app, T0 + 60_000)["id"].as_str().unwrap().to_owned();
    app.finish_json(&b, 1000, "");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &a);
    sc.status = meeting::status::Status::Failed;
    sc.failed_at = None;
    meeting::sidecar::write(&dir, &sc).unwrap();
    let mut sc = read_sidecar(&dir, &b);
    sc.failed_at = Some(meeting::status::FailedAt::Summarising); // stray on `recorded`
    meeting::sidecar::write(&dir, &sc).unwrap();
    let rows = parse(&app.list_recent_json(5));
    let row = |id: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (row(&a)["status"].clone(), row(&a)["failedAt"].clone()),
        (json!("failed"), json!("transcribing"))
    );
    assert_eq!(
        (row(&b)["status"].clone(), row(&b)["failedAt"].clone()),
        (json!("recorded"), json!(null))
    );
    let v = parse(&app.get_meeting_json(&a));
    assert_eq!(v["meeting"]["failedAt"], "transcribing");
    // Recovery rewrites the files themselves, without reporting them as recovered.
    assert_eq!(recover(&app, true)["recovered"], json!([]));
    let sa = read_sidecar(&dir, &a);
    assert_eq!(sa.failed_at, Some(meeting::status::FailedAt::Transcribing));
    assert_eq!(read_sidecar(&dir, &b).failed_at, None);
}

#[test]
fn zero_segment_recording_is_flagged() {
    let e = Env::new();
    let app = e.app();
    let id = begin(&app, T0)["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 0, "");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    sc.segments.clear();
    meeting::sidecar::write(&dir, &sc).unwrap();
    let r = recover(&app, false);
    assert_eq!(r["noAudio"], json!([id]));
    assert_eq!(
        read_sidecar(&dir, &id).error.as_deref(),
        Some("No audio was recorded")
    );
    assert!(
        recover(&app, false).get("noAudio").is_none(),
        "flagged once"
    );
    // Transcribe fails cleanly without sending anything.
    let keys = e.data.path().join("no-keys");
    fs::create_dir_all(&keys).unwrap();
    fs::write(
        keys.join("copilot-key-openai.txt"),
        "provider=openai\nkey=sk-x\nmodel=m\n",
    )
    .unwrap();
    let out = parse(&app.run_job_json(&id, meeting::app::Job::Transcribe));
    let m = &out["meeting"];
    assert_eq!(
        (m["status"].as_str(), m["failedAt"].as_str()),
        (Some("failed"), Some("transcribing"))
    );
    assert_eq!(m["error"], "No audio was recorded");
}
