//! The process-global API (`CORE`, `RUNTIME`), in its own binary so no other test sees the
//! initialised state. One test, in order: before init, bad init, init, init again, after init.
use meeting::api;
use serde_json::{Value, json};

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn init_then_test_connection() {
    assert_eq!(
        parse(&api::test_connection("summarise", "openai")),
        json!({"error": "nativeInit has not run"})
    );
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
}
