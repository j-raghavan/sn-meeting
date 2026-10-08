//! F3-FR4 / EC4: audio with no sidecar (the spike's files, or a sidecar deleted on the desktop)
//! is listed as `recorded`, timed by its file name read as device local time.
mod libcommon;

use libcommon::*;
use meeting::library::Source;
use meeting::status::Status;
use serde_json::json;
use std::fs;

const SPIKE_A: &str = "meeting-20261007-211359";
const SPIKE_B: &str = "meeting-20261007-211706";

fn spike(dir: &std::path::Path) {
    fs::write(dir.join(format!("{SPIKE_A}.m4a")), b"aac").unwrap();
    fs::write(dir.join(format!("{SPIKE_B}.m4a")), b"aac").unwrap();
}

#[test]
fn spike_files_are_single_segment_recorded_meetings() {
    let e = Env::new();
    spike(&e.folder("EXPORT/sn-meeting"));
    let app = e.app();
    let list = parse(&app.list_recent_json(5));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(
        list[0],
        json!({"id": SPIKE_B, "startedAt": "2026-10-07T21:17:06-07:00", "startedAtMs": T0,
               "durationMs": null, "status": "recorded", "failedAt": null, "title": null,
               "segmentCount": 1, "openTodoCount": 0, "source": "legacy", "folder": e.export()})
    );
    assert_eq!(list[1]["id"], SPIKE_A);
    assert!(
        !e.export().join(format!("{SPIKE_A}.json")).exists(),
        "listing writes nothing"
    );
}

#[test]
fn the_file_name_is_read_in_the_device_zone() {
    let e = Env::new();
    spike(&e.folder("EXPORT/sn-meeting"));
    // +05:30: 21:17:06 local is 15:47:06 UTC.
    let list = parse(&e.app_tz(5 * 3600 + 1800).list_recent_json(5));
    assert_eq!(list[0]["startedAt"], "2026-10-07T21:17:06+05:30");
    assert_eq!(
        list[0]["startedAtMs"],
        T0 - 7 * 3600 * 1000 - 5 * 3600 * 1000 - 1800 * 1000
    );
}

#[test]
fn legacy_and_sidecar_meetings_sort_together() {
    let e = Env::new();
    let old = e.folder("EXPORT/sn-meeting");
    spike(&old);
    let app = e.app();
    app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#);
    // A newer meeting in the new folder sorts before both spike files.
    let id = begin(&app, T0 + 60_000)["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1000, "");
    let list = parse(&app.list_recent_json(5));
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [id.as_str(), SPIKE_B, SPIKE_A]);
}

#[test]
fn orphan_segments_list_in_order() {
    // A sidecar deleted on the desktop (EC7): its segments still list, in file order.
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    for n in [2, 1, 10] {
        fs::write(dir.join(format!("{SPIKE_B}-{n:02}.m4a")), b"aac").unwrap();
    }
    let app = e.app();
    let v = parse(&app.get_meeting_json(SPIKE_B));
    assert_eq!(v["source"], "legacy");
    let files: Vec<&str> = v["meeting"]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            format!("{SPIKE_B}-01.m4a"),
            format!("{SPIKE_B}-02.m4a"),
            format!("{SPIKE_B}-10.m4a")
        ]
    );
    assert_eq!(v["meeting"]["mic"], json!(null));
    assert_eq!(
        v["segmentPaths"][0],
        json!(dir.join(format!("{SPIKE_B}-01.m4a")))
    );
}

#[test]
fn the_first_write_creates_a_recorded_sidecar() {
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    spike(&dir);
    let z = tz(PDT);
    let lib = library(&[&dir], &z);
    lib.close_segment(SPIKE_A, 0, 120_000).unwrap();
    let sc = read_sidecar(&dir, SPIKE_A);
    assert_eq!(sc.status, Status::Recorded);
    assert_eq!(sc.started_at, "2026-10-07T21:13:59-07:00");
    assert_eq!((sc.mic.clone(), sc.audio.clone()), (None, None));
    assert_eq!(sc.segments[0].file, format!("{SPIKE_A}.m4a"));
    assert_eq!(sc.segments[0].duration_ms, Some(120_000));
    let list = lib.list_recent(5);
    let a = list.iter().find(|m| m.id == SPIKE_A).unwrap();
    assert_eq!((a.source, a.started_at_ms), (Source::Sidecar, T0 - 187_000));
}

#[test]
fn duplicate_numbers_list_once() {
    // "-001" and "-1" are not segment names (only the canonical "-01"), so one number can't
    // list twice.
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    for name in ["-01", "-001", "-1", "-0001", "-100"] {
        fs::write(dir.join(format!("{SPIKE_B}{name}.m4a")), b"aac").unwrap();
    }
    let v = parse(&e.app().get_meeting_json(SPIKE_B));
    let files: Vec<&str> = v["meeting"]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [format!("{SPIKE_B}-01.m4a"), format!("{SPIKE_B}-100.m4a")]
    );
    assert_eq!(
        meeting::sidecar::segment_number(SPIKE_B, &format!("{SPIKE_B}-001.m4a")),
        None
    );
    assert_eq!(
        meeting::sidecar::segment_number(SPIKE_B, &format!("{SPIKE_B}-100.m4a")),
        Some(Some(100))
    );
}

#[test]
fn recovery_lists_legacy_audio_for_measuring_and_the_durations_stick() {
    // D3: legacy audio listed with no duration ("—" on Home) until the shell measures it.
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    fs::write(dir.join(format!("{SPIKE_A}.m4a")), b"aac").unwrap();
    for n in [1, 2] {
        fs::write(dir.join(format!("{SPIKE_B}-{n:02}.m4a")), b"aac").unwrap();
    }
    fs::write(dir.join("meeting-20261001-080000.m4a"), b"").unwrap(); // nothing to measure
    let app = e.app();
    let r = parse(&app.recover_json(false));
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    assert_eq!(
        r["measure"],
        json!([
            {"id": SPIKE_A, "index": 0, "path": path(&format!("{SPIKE_A}.m4a"))},
            {"id": SPIKE_B, "index": 0, "path": path(&format!("{SPIKE_B}-01.m4a"))},
            {"id": SPIKE_B, "index": 1, "path": path(&format!("{SPIKE_B}-02.m4a"))},
        ])
    );
    assert!(
        !dir.join(format!("{SPIKE_A}.json")).exists(),
        "recovery writes nothing for legacy audio"
    );
    // The shell measures each file and closes it, which writes the first sidecar.
    for (id, i, ms) in [
        (SPIKE_A, 0, 120_000),
        (SPIKE_B, 0, 900_000),
        (SPIKE_B, 1, 300_000),
    ] {
        assert_eq!(parse(&app.close_segment_json(id, i, ms))["ok"], true);
    }
    let list = parse(&app.list_recent_json(5));
    let row = |id: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(row(SPIKE_A)["durationMs"], 120_000);
    assert_eq!(row(SPIKE_B)["durationMs"], 1_200_000);
    assert_eq!(row(SPIKE_B)["source"], "sidecar");
    let sc = read_sidecar(&dir, SPIKE_B);
    assert_eq!(
        sc.segments[1].start_offset_ms, 900_000,
        "a legacy segment follows the one before"
    );
    // Idempotent: nothing is left to measure.
    assert_eq!(parse(&app.recover_json(false))["measure"], json!([]));
}
