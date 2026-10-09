//! F3-FR3: list_recent and get, across folders, newest first.
mod libcommon;

use libcommon::*;
use meeting::library::Source;
use serde_json::{Value, json};
use std::fs;

const MIN: i64 = 60_000;

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}

/// `n` finished meetings one minute apart from T0.
fn meetings(app: &meeting::app::App, n: i64) -> Vec<String> {
    (0..n)
        .map(|i| {
            let id = begin(app, T0 + i * MIN)["id"].as_str().unwrap().to_owned();
            app.finish_json(&id, 30_000, "");
            id
        })
        .collect()
}

#[test]
fn the_five_newest_of_seven_newest_first() {
    // F3-AC1 / F6-AC1.
    let e = Env::new();
    let app = e.app();
    let all = meetings(&app, 7);
    let list = parse(&app.list_recent_json(5));
    let want: Vec<String> = all.iter().rev().take(5).cloned().collect();
    assert_eq!(ids(&list), want);
    let first = &list[0];
    assert_eq!(
        *first,
        json!({"id": all[6], "startedAt": "2026-10-07T21:23:06-07:00", "startedAtMs": T0 + 6 * MIN,
               "durationMs": 30_000, "status": "recorded", "failedAt": null, "title": null,
               "segmentCount": 1, "openTodoCount": 0, "source": "sidecar", "folder": e.export(),
               "spend": {"usd": 0.0, "unpriced": 0, "records": 0, "label": "—"}})
    );
    assert_eq!(parse(&app.list_recent_json(0)), json!([]));
    assert_eq!(ids(&parse(&app.list_recent_json(100))).len(), 7);
    assert_eq!(
        parse(&app.list_recent_json(-1)),
        json!({"error": "limit must be >= 0"})
    );
}

#[test]
fn meetings_in_every_save_folder_are_listed() {
    // F10-AC3: after moving to Document/Meetings, the EXPORT meetings are still listed.
    let e = Env::new();
    let app = e.app();
    let old = meetings(&app, 2);
    app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#);
    let new = begin(&app, T0 + 10 * MIN)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let list = parse(&app.list_recent_json(5));
    assert_eq!(
        ids(&list),
        vec![new.clone(), old[1].clone(), old[0].clone()]
    );
    assert_eq!(list[0]["status"], "recording", "listing while recording");
    assert_eq!(
        list[0]["folder"],
        json!(e.root.path().join("Document/Meetings"))
    );
    assert_eq!(list[1]["folder"], json!(e.export()));
}

#[test]
fn ties_and_offsets_sort_by_the_instant() {
    let e = Env::new();
    let z = tz(PDT);
    let dir = e.folder("EXPORT/sn-meeting");
    let lib = library(&[&dir], &z);
    let a = lib.begin(T0, mic(), audio(), None).unwrap().id;
    let b = lib.begin(T0, mic(), audio(), None).unwrap().id;
    // A meeting recorded in another zone sorts by its instant, not its wall clock.
    let mut sc = read_sidecar(&dir, &b);
    sc.started_at = "2026-10-08T06:17:05+02:00".into(); // = T0 - 1 s
    meeting::sidecar::write(&dir, &sc).unwrap();
    let list = lib.list_recent(5);
    assert_eq!(
        list.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
        [a.clone(), b.clone()]
    );
    assert_eq!(list[1].started_at_ms, T0 - 1000);
    // A started_at we can't parse falls back to the id's timestamp.
    sc.started_at = "yesterday".into();
    meeting::sidecar::write(&dir, &sc).unwrap();
    let list = lib.list_recent(5);
    assert_eq!(list[0].id, b, "same instant as a: the later id first");
    assert_eq!(list[0].started_at_ms, T0);
}

#[test]
fn stray_tmp_and_foreign_files_are_ignored_and_corrupt_sidecars_listed() {
    let e = Env::new();
    let app = e.app();
    let id = meetings(&app, 1).remove(0);
    let dir = e.export();
    fs::write(dir.join(format!("{id}.json.tmp")), "{half").unwrap();
    for other in [
        "notes.txt",
        "meeting-x.json",
        "meeting-20261007-211706.transcript.md",
        "Recording.m4a",
    ] {
        fs::write(dir.join(other), b"x").unwrap();
    }
    fs::create_dir(dir.join("meeting-20261007-999999.json")).unwrap();
    let bad = "meeting-20261006-090000";
    fs::write(dir.join(format!("{bad}.json")), "{not json").unwrap();
    fs::write(dir.join(format!("{bad}-01.m4a")), b"aac").unwrap();
    let list = parse(&app.list_recent_json(10));
    assert_eq!(ids(&list), vec![id.clone(), bad.to_owned()]);
    assert_eq!(list[1]["source"], "corrupt");
    assert_eq!(list[1]["status"], "recorded");
    assert_eq!(list[1]["startedAt"], "2026-10-06T09:00:00-07:00");
    assert_eq!(list[1]["durationMs"], json!(null));
    assert_eq!(list[1]["segmentCount"], 1);
    let r = parse(&app.get_meeting_json(bad));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("sidecar is not valid JSON"),
        "{r}"
    );
}

#[test]
fn a_missing_or_unreadable_folder_lists_what_remains() {
    let e = Env::new();
    let app = e.app();
    assert_eq!(parse(&app.list_recent_json(5)), json!([]), "no folder yet");
    meetings(&app, 1);
    app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#);
    fs::remove_dir(e.root.path().join("Document/Meetings")).unwrap();
    assert_eq!(ids(&parse(&app.list_recent_json(5))).len(), 1);
}

#[test]
fn get_returns_the_whole_sidecar_and_absolute_paths() {
    let e = Env::new();
    let app = e.app();
    let id = meetings(&app, 1).remove(0);
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    sc.extra.insert("future".into(), json!({"a": 1}));
    sc.transcript_file = Some(format!("{id}.transcript.md"));
    meeting::sidecar::write(&dir, &sc).unwrap();
    let v = parse(&app.get_meeting_json(&id));
    assert_eq!(v["meeting"]["future"], json!({"a": 1}));
    assert_eq!(v["meeting"]["id"], id);
    assert_eq!(v["source"], json!(Source::Sidecar));
    assert_eq!(v["dir"], json!(dir));
    assert_eq!(v["segmentPaths"], json!([dir.join(format!("{id}-01.m4a"))]));
    assert_eq!(
        v["transcriptPath"],
        json!(dir.join(format!("{id}.transcript.md")))
    );
    assert_eq!(
        parse(&app.get_meeting_json("meeting-20200101-000000")),
        json!({"error": "meeting not found"})
    );
    assert_eq!(
        parse(&app.get_meeting_json("../x")),
        json!({"error": "invalid meeting id"})
    );
}
