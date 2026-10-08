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
