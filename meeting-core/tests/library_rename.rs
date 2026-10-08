//! F7-FR5: naming a meeting; clearing returns it to untitled.
mod libcommon;

use libcommon::*;
use meeting::sidecar::TitleSource;
use serde_json::json;
use std::fs;

#[test]
fn a_user_title_is_kept_and_listed() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    app.finish_json(ID0, 1000, "");
    assert_eq!(
        parse(&app.rename_json(ID0, "  Hiring sync  ")),
        json!({"ok": true})
    );
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!(
        (sc.title.as_deref(), sc.title_source),
        (Some("Hiring sync"), Some(TitleSource::User))
    );
    // F7-AC3, the part this milestone owns: a restart still shows it.
    assert_eq!(
        parse(&e.app().list_recent_json(1))[0]["title"],
        "Hiring sync"
    );
    let t200 = "é".repeat(200);
    assert_eq!(parse(&app.rename_json(ID0, &t200)), json!({"ok": true}));
}

#[test]
fn clearing_returns_to_untitled() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    app.rename_json(ID0, "Standup");
    for blank in ["", "   "] {
        app.rename_json(ID0, "Standup");
        assert_eq!(parse(&app.rename_json(ID0, blank)), json!({"ok": true}));
        let sc = read_sidecar(&e.export(), ID0);
        assert_eq!((sc.title, sc.title_source), (None, None), "{blank:?}");
    }
}

#[test]
fn bad_titles_are_refused_and_change_nothing() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    app.rename_json(ID0, "Kept");
    assert_eq!(
        parse(&app.rename_json(ID0, "a\nb")),
        json!({"error": "title can't contain control characters"})
    );
    assert_eq!(
        parse(&app.rename_json(ID0, &"x".repeat(201))),
        json!({"error": "title is too long"})
    );
    assert_eq!(
        read_sidecar(&e.export(), ID0).title.as_deref(),
        Some("Kept")
    );
    assert_eq!(
        parse(&app.rename_json("meeting-20200101-000000", "x")),
        json!({"error": "meeting not found"})
    );
}

#[test]
fn renaming_legacy_audio_creates_its_sidecar() {
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    fs::write(dir.join("meeting-20261007-211359.m4a"), b"aac").unwrap();
    let app = e.app();
    assert_eq!(
        parse(&app.rename_json("meeting-20261007-211359", "Spike")),
        json!({"ok": true})
    );
    let list = parse(&app.list_recent_json(1));
    assert_eq!(
        (list[0]["title"].clone(), list[0]["source"].clone()),
        (json!("Spike"), json!("sidecar"))
    );
    assert_eq!(list[0]["status"], "recorded");
}
