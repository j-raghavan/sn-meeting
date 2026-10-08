//! F7-FR4: delete removes a meeting's own audio, sidecar and transcript, and nothing else.
mod libcommon;

use libcommon::*;
use serde_json::json;
use std::fs;

const B: &str = "meeting-20261007-211706b";

#[test]
fn only_this_meetings_files_go() {
    let e = Env::new();
    let app = e.app();
    let dir = e.export();
    begin(&app, T0);
    app.add_segment_json(ID0, dir.join(format!("{ID0}-02.m4a")).to_str().unwrap(), 5);
    app.finish_json(ID0, 10, "");
    begin(&app, T0); // …b, same second
    app.finish_json(B, 10, "");
    for f in [
        format!("{ID0}-01.m4a"),
        format!("{ID0}-02.m4a"),
        format!("{ID0}.transcript.md"),
        format!("{ID0}.json.tmp"),
        format!("{ID0}.json.77-3.tmp"),
        format!("{B}.json.77-4.tmp"),
        format!("{ID0}.transcript.md.77-5.tmp"),
        format!("{B}.transcript.md.77-6.tmp"),
        format!("{B}-01.m4a"),
        format!("{B}.transcript.md"),
        "notes.txt".into(),
        format!("{ID0}.m4a.bak"),
    ] {
        fs::write(dir.join(f), b"x").unwrap();
    }
    let r = parse(&app.delete_json(ID0));
    assert_eq!(
        r,
        json!({"ok": true, "removed": [format!("{ID0}-01.m4a"), format!("{ID0}-02.m4a"),
            format!("{ID0}.transcript.md"), format!("{ID0}.transcript.md.77-5.tmp"),
            format!("{ID0}.json.77-3.tmp"), format!("{ID0}.json.tmp"), format!("{ID0}.json")]})
    );
    let mut left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            format!("{ID0}.m4a.bak"),
            format!("{B}-01.m4a"),
            format!("{B}.json"),
            format!("{B}.json.77-4.tmp"),
            format!("{B}.transcript.md"),
            format!("{B}.transcript.md.77-6.tmp"),
            "notes.txt".into()
        ]
    );
    let ids: Vec<String> = parse(&app.list_recent_json(5))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().into())
        .collect();
    assert_eq!(ids, [B]);
    assert_eq!(
        parse(&app.delete_json(ID0)),
        json!({"error": "meeting not found"})
    );
}

#[test]
fn not_while_recording_or_transcribing() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        parse(&app.delete_json(ID0)),
        json!({"error": "meeting is recording"})
    );
    app.finish_json(ID0, 10, "");
    let job = app.jobs.start(ID0).unwrap(); // a job in flight, as run_job_json holds it
    assert_eq!(
        parse(&app.delete_json(ID0)),
        json!({"error": "meeting is being transcribed"})
    );
    drop(job);
    assert_eq!(parse(&app.delete_json(ID0))["ok"], true);
}

#[test]
fn legacy_corrupt_and_previous_folder_meetings() {
    let e = Env::new();
    let old = e.folder("EXPORT/sn-meeting");
    fs::write(old.join("meeting-20261007-211359.m4a"), b"spike").unwrap();
    let bad = "meeting-20261006-090000";
    fs::write(old.join(format!("{bad}.json")), "{broken").unwrap();
    fs::write(old.join(format!("{bad}-01.m4a")), b"aac").unwrap();
    let app = e.app();
    app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#);
    assert_eq!(
        parse(&app.delete_json("meeting-20261007-211359"))["removed"],
        json!(["meeting-20261007-211359.m4a"])
    );
    assert_eq!(
        parse(&app.delete_json(bad))["removed"],
        json!([format!("{bad}-01.m4a"), format!("{bad}.json")])
    );
    assert_eq!(fs::read_dir(&old).unwrap().count(), 0);
}

#[test]
fn a_sidecar_naming_other_files_cannot_widen_the_delete() {
    let e = Env::new();
    let app = e.app();
    let dir = e.export();
    begin(&app, T0);
    app.finish_json(ID0, 10, "");
    let victim = e.folder("Document").join("keep.m4a");
    fs::write(&victim, b"keep").unwrap();
    // A hand-edited sidecar pointing elsewhere is unreadable, so only scanned names go.
    let path = dir.join(format!("{ID0}.json"));
    let txt = fs::read_to_string(&path)
        .unwrap()
        .replace(&format!("{ID0}-01.m4a"), "../../Document/keep.m4a");
    fs::write(&path, txt).unwrap();
    assert_eq!(
        parse(&app.delete_json(ID0))["removed"],
        json!([format!("{ID0}.json")])
    );
    assert!(victim.exists());
    assert_eq!(
        parse(&app.delete_json("../Document/keep")),
        json!({"error": "invalid meeting id"})
    );
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_removed_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    app.finish_json(ID0, 10, "");
    fs::write(e.export().join(format!("{ID0}-01.m4a")), b"aac").unwrap();
    fs::set_permissions(e.export(), fs::Permissions::from_mode(0o500)).unwrap();
    let r = parse(&app.delete_json(ID0));
    fs::set_permissions(e.export(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(r, json!({"error": "storage: permission denied"}));
    assert!(
        e.export().join(format!("{ID0}.json")).exists(),
        "the sidecar goes last"
    );
}
