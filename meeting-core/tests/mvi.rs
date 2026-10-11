//! ADR D10, minimum-viable input: a fresh install with no key files, no settings.json, no
//! templates folder and no network still records, lists and plays a meeting. Everything goes
//! through the App methods the JNI exports call.
mod libcommon;

use libcommon::*;
use serde_json::json;
use std::path::Path;

#[test]
fn a_fresh_install_records_lists_and_plays_a_meeting() {
    let e = Env::new();
    assert_eq!(
        std::fs::read_dir(e.root.path()).unwrap().count(),
        0,
        "empty storage"
    );
    let app = e.app();

    // Settings: defaults, every provider missing its key, still usable.
    let s = parse(&app.get_settings_json());
    assert_eq!(s["origin"], "defaults");
    for p in ["openai", "gemini", "anthropic"] {
        assert_eq!(s["keys"][p]["status"], "missing", "{p}");
    }
    assert_eq!(s["effective"]["transcribe"]["provider"], "openai");
    assert!(
        !e.data.path().join("settings.json").exists(),
        "nothing written until a change"
    );

    // Recovery on launch finds nothing and fails nothing.
    assert_eq!(
        parse(&app.recover_json(false)),
        json!({"recovered": [], "measure": [], "flagged": []})
    );
    assert_eq!(parse(&app.list_recent_json(5)), json!([]));
    assert_eq!(parse(&app.open_todos_json()), json!([]));

    // Record: begin, the shell writes audio, close, finish.
    let b = begin(&app, T0);
    let path = b["firstSegmentPath"].as_str().unwrap().to_owned();
    assert!(
        path.starts_with(e.export().to_str().unwrap()),
        "the default save folder is created"
    );
    std::fs::write(&path, b"aac frames").unwrap();
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, 5000)),
        json!({"ok": true})
    );
    assert_eq!(parse(&app.finish_json(ID0, 5000, "")), json!({"ok": true}));

    // List and play.
    let list = parse(&app.list_recent_json(5));
    assert_eq!(
        (list[0]["id"].clone(), list[0]["status"].clone()),
        (json!(ID0), json!("recorded"))
    );
    let view = parse(&app.get_meeting_json(ID0));
    assert!(Path::new(view["segmentPaths"][0].as_str().unwrap()).is_file());

    // The enhancements degrade to a message instead of blocking anything.
    let t = parse(&app.test_connection("transcribe", "openai"));
    assert_eq!(
        (t["status"].clone(), t["detail"].clone()),
        (
            json!("no_key"),
            json!("Import copilot-key-openai.txt in Settings, or add it to MyStyle/SnCopilot")
        )
    );
    assert_eq!(
        parse(&app.set_template_json(ID0, "general")),
        json!({"ok": true})
    );
    assert!(
        !e.root.path().join("MyStyle").exists(),
        "no templates folder needed or made"
    );

    // A later restart with the same files sees the same meeting.
    assert_eq!(parse(&e.app().list_recent_json(5))[0]["id"], ID0);
}
