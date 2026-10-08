//! F3-FR6 and the Data Model: sidecar schema v1, unknown fields at every level, safe names.
use meeting::sidecar::{
    self, SegmentFlag, Sidecar, SidecarError, is_valid_id, segment_file_name, segment_number,
};
use meeting::status::{FailedAt, Status};
use serde_json::{Value, json};
use std::fs;

const ID: &str = "meeting-20261007-211706";

/// The spec's example, with an unknown field at every level.
fn spec_example() -> Value {
    json!({
        "schema": 1, "id": ID, "startedAt": "2026-10-07T21:17:06-07:00",
        "durationMs": 0, "status": "recording", "failedAt": null,
        "title": null, "titleSource": null, "templateId": "general",
        "providers": {
            "transcribe": {"provider": "openai", "model": "gpt-transcribe", "p1": 1},
            "summarise": {"provider": "anthropic", "model": "claude-opus-5-5"},
            "p0": true
        },
        "summary": null, "decisions": [],
        "sections": [{"heading": "Blockers", "body": "x", "s1": [1]}],
        "mic": {"kind": "usb", "name": "USB-Audio - USB Audio", "m1": "a"},
        "audio": {"codec": "aac-lc", "sampleRateHz": 44100, "bitrateBps": 96000, "channels": 1, "a1": 2},
        "segments": [{"file": "meeting-20261007-211706-01.m4a", "startOffsetMs": 0,
                      "durationMs": null, "text": null, "zz": 1}],
        "transcriptFile": null, "error": null, "note": null,
        "todos": [{"id": "7f3a", "text": "Send the deck to Priya", "owner": "me",
                   "due": "2026-10-09", "done": false, "firstSeen": "2026-10-07T22:01:00-07:00",
                   "t1": null}],
        "future": {"a": [1]}
    })
}

#[test]
fn the_spec_example_round_trips_with_unknown_fields_everywhere() {
    let v = spec_example();
    let sc: Sidecar = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(sc.status, Status::Recording);
    assert_eq!(sc.segments[0].extra["zz"], 1);
    assert_eq!(serde_json::to_value(&sc).unwrap(), v);
    let d = tempfile::tempdir().unwrap();
    sidecar::write(d.path(), &sc).unwrap();
    let back: Value =
        serde_json::from_slice(&fs::read(d.path().join(format!("{ID}.json"))).unwrap()).unwrap();
    assert_eq!(back, v);
    assert_eq!(sidecar::read(d.path(), ID).unwrap(), sc);
}

#[test]
fn a_minimal_sidecar_writes_every_field_and_a_flag_only_when_set() {
    let min = json!({"schema": 1, "id": ID, "startedAt": "2026-10-07T21:17:06-07:00",
        "status": "failed", "failedAt": "summarising",
        "segments": [{"file": format!("{ID}-01.m4a"), "startOffsetMs": 0}],
        "todos": [{"id": "a", "text": "t"}]});
    let mut sc: Sidecar = serde_json::from_value(min).unwrap();
    assert_eq!(sc.failed_at, Some(FailedAt::Summarising));
    let v = serde_json::to_value(&sc).unwrap();
    for k in [
        "durationMs",
        "title",
        "titleSource",
        "templateId",
        "summary",
        "mic",
        "audio",
        "transcriptFile",
        "error",
        "note",
        "decisions",
        "sections",
    ] {
        assert!(v.get(k).is_some(), "{k} missing");
    }
    assert_eq!(
        v["providers"],
        json!({"transcribe": null, "summarise": null})
    );
    assert_eq!(
        v["segments"][0],
        json!({"file": format!("{ID}-01.m4a"), "startOffsetMs": 0, "durationMs": null, "text": null})
    );
    assert_eq!(v["todos"][0]["firstSeen"], "");
    sc.segments[0].flag = Some(SegmentFlag::Empty);
    assert_eq!(
        serde_json::to_value(&sc).unwrap()["segments"][0]["flag"],
        "empty"
    );
}

#[test]
fn ids_have_one_shape() {
    for ok in [
        ID,
        "meeting-20261007-211706b",
        "meeting-20261007-211706z",
        "meeting-19991231-235959",
    ] {
        assert!(is_valid_id(ok), "{ok}");
    }
    for bad in [
        "meeting-20261007-211706a",
        "meeting-20261007-211706bb",
        "meeting-2026100-211706",
        "meeting-20261007_211706",
        "../meeting-20261007-211706",
        "meeting-20261007-211706/",
        "Meeting-20261007-211706",
        "meeting-20261007-21170x",
        "",
        "meeting-",
    ] {
        assert!(!is_valid_id(bad), "{bad:?}");
    }
}

#[test]
fn segment_and_transcript_names_are_confined() {
    assert_eq!(segment_number(ID, &format!("{ID}.m4a")), Some(None));
    assert_eq!(segment_number(ID, &format!("{ID}-02.m4a")), Some(Some(2)));
    assert_eq!(
        segment_number(ID, &format!("{ID}-123.m4a")),
        Some(Some(123))
    );
    for bad in [
        "../x.m4a",
        &format!("{ID}-00.m4a"),
        &format!("{ID}-1.m4a"),
        &format!("{ID}-01.m4a/../../a"),
        &format!("/etc/{ID}.m4a"),
        &format!("{ID}b-01.m4a"),
        &format!("{ID}-0a.m4a"),
        &format!("{ID}-01.mp3"),
    ] {
        assert_eq!(segment_number(ID, bad), None, "{bad}");
    }
    assert_eq!(segment_file_name(ID, 3), format!("{ID}-03.m4a"));
    assert_eq!(
        sidecar::transcript_file_name(ID),
        format!("{ID}.transcript.md")
    );
    let mut sc: Sidecar = serde_json::from_value(spec_example()).unwrap();
    let d = std::path::Path::new("/x");
    assert_eq!(sc.segment_paths(d), vec![d.join(format!("{ID}-01.m4a"))]);
    sc.transcript_file = Some(format!("{ID}.transcript.md"));
    sc.check_names().unwrap();
    sc.transcript_file = Some("../../evil.md".into());
    assert!(matches!(sc.check_names(), Err(SidecarError::UnsafeName(_))));
}

#[test]
fn read_refuses_what_is_not_ours() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join(format!("{ID}.json"));
    let mut v = spec_example();
    let err = |v: &Value| {
        fs::write(&path, v.to_string()).unwrap();
        sidecar::read(d.path(), ID).unwrap_err().to_string()
    };
    v["id"] = json!("meeting-20261007-211707");
    assert_eq!(
        err(&v),
        "sidecar id \"meeting-20261007-211707\" does not match its file name"
    );
    v["id"] = json!(ID);
    v["segments"][0]["file"] = json!("../../../etc/passwd");
    assert_eq!(
        err(&v),
        "unsafe file name in sidecar: \"../../../etc/passwd\""
    );
    v = spec_example();
    v["schema"] = json!(2);
    assert_eq!(err(&v), "sidecar schema 2 is newer than this app");
    fs::write(&path, "{nope").unwrap();
    assert!(
        sidecar::read(d.path(), ID)
            .unwrap_err()
            .to_string()
            .starts_with("sidecar is not valid JSON")
    );
    fs::write(&path, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    assert_eq!(
        sidecar::read(d.path(), ID).unwrap_err().to_string(),
        "sidecar unreadable: invalid data"
    );
    assert!(matches!(
        sidecar::read(d.path(), "meeting-20261007-211708"),
        Err(SidecarError::Io(_))
    ));
    assert!(matches!(
        sidecar::read(d.path(), "../x"),
        Err(SidecarError::BadId)
    ));
}

#[test]
fn write_refuses_a_bad_id_or_names_and_survives_a_stray_tmp() {
    let d = tempfile::tempdir().unwrap();
    let mut sc: Sidecar = serde_json::from_value(spec_example()).unwrap();
    sidecar::write(d.path(), &sc).unwrap();
    // F3-AC3: an interrupted write leaves only a .tmp; the previous sidecar reads intact.
    fs::write(
        d.path().join(format!("{ID}.json.tmp")),
        "{\"schema\": 1, \"id\"",
    )
    .unwrap();
    assert_eq!(sidecar::read(d.path(), ID).unwrap(), sc);
    sc.segments[0].file = "../x.m4a".into();
    assert_eq!(
        sidecar::write(d.path(), &sc).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    sc.segments.clear();
    sc.id = "../evil".into();
    assert_eq!(
        sidecar::write(d.path(), &sc).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert!(!d.path().join("../evil.json").exists());
}
