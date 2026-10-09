//! The process-global API (`APP`, `RUNTIME`), in its own binary so no other test sees the
//! initialised state. One test, in order: before init, bad init, init, init again, after init.
use meeting::api;
use serde_json::{Value, json};

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

/// Every export that needs `nativeInit`, called with harmless arguments.
fn app_exports() -> Vec<(&'static str, String)> {
    vec![
        (
            "testConnection",
            api::test_connection("summarise", "openai"),
        ),
        ("getSettings", api::get_settings()),
        ("setSettings", api::set_settings("{}")),
        ("beginMeeting", api::begin_meeting(0, "{}", "{}")),
        ("addSegment", api::add_segment("x", "y", 0)),
        ("closeSegment", api::close_segment("x", 0, 0)),
        ("finishMeeting", api::finish_meeting("x", 0, "")),
        ("listRecent", api::list_recent(5)),
        ("getMeeting", api::get_meeting("x")),
        ("openTodos", api::open_todos()),
        ("setTodoDone", api::set_todo_done("x", "y", true)),
        ("renameMeeting", api::rename_meeting("x", "y")),
        (
            "setMeetingTemplate",
            api::set_meeting_template("x", "general"),
        ),
        ("deleteMeeting", api::delete_meeting("x")),
        ("recover", api::recover(true)),
        ("transcribe", api::transcribe("x")),
        ("transcribeAgain", api::transcribe_again("x")),
        ("summarise", api::summarise("x")),
        ("cancel", api::cancel("x")),
        ("listTemplates", api::list_templates()),
        ("getTemplate", api::get_template("general")),
        ("saveTemplate", api::save_template("{}")),
        ("resetTemplate", api::reset_template("../x")),
        ("previewTemplate", api::preview_template("{}")),
        ("getRates", api::get_rates()),
        ("spend", api::spend("2026-10", "2026-10")),
        ("repriceAll", api::reprice_all(true)),
        ("setRates", api::set_rates("[]")),
        ("search", api::search("x", "all", 0)),
        ("indexStatus", api::index_status()),
        ("indexUpdate", api::index_update()),
        ("rebuildIndex", api::rebuild_index()),
    ]
}

#[test]
fn init_then_the_app_exports() {
    for (name, reply) in app_exports() {
        assert_eq!(
            parse(&reply),
            json!({"error": "nativeInit has not run"}),
            "{name}"
        );
    }
    for bad in ["", "relative/dir"] {
        assert_eq!(
            parse(&api::init(bad)),
            json!({"error": "appDataDir must be an absolute path"})
        );
    }
    let first = parse(&api::init("/data/user/0/dev.jraghavan.snmeeting/files"));
    assert_eq!(
        first,
        json!({"ok": true, "version": meeting::version(), "appDataDir": "/data/user/0/dev.jraghavan.snmeeting/files"})
    );
    assert_eq!(
        parse(&api::init("/elsewhere")),
        first,
        "idempotent: the first dir is kept"
    );
    for (name, reply) in app_exports() {
        assert_ne!(
            parse(&reply),
            json!({"error": "nativeInit has not run"}),
            "{name}"
        );
    }
    // The device key folder does not exist on the host, so this is the F4-FR5 "add" message.
    let r = parse(&api::test_connection("summarise", "openai"));
    assert_eq!(
        (r["status"].clone(), r["detail"].clone()),
        (
            json!("no_key"),
            json!("Add copilot-key-openai.txt to MyStyle/SnCopilot")
        )
    );
    assert_eq!(
        parse(&api::test_connection("x", "openai")),
        json!({"error": "unknown role"})
    );
    // Jobs check the id before anything else (no key folder is read for a bad one).
    assert_eq!(
        parse(&api::transcribe("x")),
        json!({"error": "invalid meeting id"})
    );
    assert_eq!(
        parse(&api::summarise("../x")),
        json!({"error": "invalid meeting id"})
    );
    assert_eq!(parse(&api::cancel("x")), json!({"cancelled": false}));
    assert_eq!(
        parse(&api::reset_template("../x")),
        json!({"error": "invalid template id"})
    );
    // nativeInit loaded settings: none on this host, so defaults and no keys.
    let s = parse(&api::get_settings());
    assert_eq!(s["origin"], "defaults");
    assert_eq!(s["keys"]["openai"]["status"], "missing");
    assert_eq!(
        s["effective"]["saveFolderPath"],
        "/storage/emulated/0/EXPORT/sn-meeting"
    );
    assert_eq!(
        parse(&api::set_settings("[]"))["error"],
        "settings must be a JSON object"
    );
}

/// Every Kotlin external but nativeVersion, nativeInit and nativeUpdateDecide (F14-FR5: a pure
/// comparison) needs the app, so each must be in `app_exports` (checked before and after init
/// above).
#[test]
fn app_exports_cover_the_bridge() {
    let kt = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/src/main/java/dev/jraghavan/snmeeting/NativeBridge.kt"),
    )
    .unwrap();
    let mut bridge: Vec<String> = kt
        .lines()
        .filter_map(|l| l.trim().strip_prefix("external fun native"))
        .map(|rest| {
            let name = rest.split('(').next().unwrap();
            let mut c = name.chars();
            let first = c.next().unwrap().to_ascii_lowercase();
            std::iter::once(first).chain(c).collect()
        })
        .filter(|n: &String| !["version", "init", "updateDecide"].contains(&n.as_str()))
        .collect();
    bridge.sort();
    // Names only: calling app_exports here before init is harmless (they all refuse).
    let mut covered: Vec<String> = app_exports()
        .into_iter()
        .map(|(n, _)| n.to_owned())
        .collect();
    covered.sort();
    assert_eq!(bridge, covered);
}
