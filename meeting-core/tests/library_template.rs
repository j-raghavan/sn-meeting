//! F7-FR2: a meeting's template, chosen on the meeting screen.
mod libcommon;

use libcommon::*;
use serde_json::json;
use std::fs;

#[test]
fn the_template_is_stored_per_meeting() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        read_sidecar(&e.export(), ID0).template_id,
        None,
        "null until chosen or transcribed"
    );
    assert_eq!(
        parse(&app.set_template_json(ID0, "standup")),
        json!({"ok": true})
    );
    assert_eq!(
        read_sidecar(&e.export(), ID0).template_id.as_deref(),
        Some("standup")
    );
    assert_eq!(
        parse(&app.get_meeting_json(ID0))["meeting"]["templateId"],
        "standup"
    );
}

#[test]
fn only_template_ids_are_accepted() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    for bad in ["", "../general", "Standup", "a b", "general.md"] {
        assert_eq!(
            parse(&app.set_template_json(ID0, bad)),
            json!({"error": "invalid template id"}),
            "{bad:?}"
        );
    }
    assert_eq!(read_sidecar(&e.export(), ID0).template_id, None);
    assert_eq!(
        parse(&app.set_template_json("meeting-20200101-000000", "general")),
        json!({"error": "meeting not found"})
    );
}

#[test]
fn a_corrupt_sidecar_is_not_rewritten() {
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    fs::write(dir.join(format!("{ID0}.json")), "{oops").unwrap();
    let r = parse(&e.app().set_template_json(ID0, "general"));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("sidecar is not valid JSON")
    );
    assert_eq!(
        fs::read_to_string(dir.join(format!("{ID0}.json"))).unwrap(),
        "{oops"
    );
}
