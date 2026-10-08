//! F11-FR6 / F10-FR6: a meeting's template and the default must be effective templates; a
//! default that has since gone is read as General.
mod libcommon;

use libcommon::*;
use meeting::app::Job;
use serde_json::json;

#[test]
fn only_effective_templates_can_be_chosen() {
    let e = Env::new();
    let app = e.app();
    let id = begin(&app, T0)["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1000, "");
    assert_eq!(
        parse(&app.set_template_json(&id, "retro")),
        json!({"error": "template not found"})
    );
    assert_eq!(
        parse(&app.set_settings_json(r#"{"defaultTemplate": "retro"}"#)),
        json!({"error": "template not found", "field": "defaultTemplate"})
    );
    // A malformed id is still the id error.
    assert_eq!(
        parse(&app.set_template_json(&id, "Retro"))["error"],
        "invalid template id"
    );
    let mut t = parse(&app.get_template_json("general"));
    t["id"] = json!("retro");
    t["name"] = json!("Retro");
    app.save_template_json(&t.to_string());
    assert_eq!(
        parse(&app.set_template_json(&id, "retro")),
        json!({"ok": true})
    );
    let s = parse(&app.set_settings_json(r#"{"defaultTemplate": "retro"}"#));
    assert_eq!(s["settings"]["defaultTemplate"], "retro");
    assert_eq!(s["effective"]["defaultTemplate"], "retro");
    // A broken YOURS file can't be chosen.
    std::fs::write(
        e.root.path().join("MyStyle/Meetings/templates/broken.md"),
        "nope",
    )
    .unwrap();
    assert_eq!(
        parse(&app.set_template_json(&id, "broken")),
        json!({"error": "template not found"})
    );
}

#[test]
fn a_deleted_default_reads_as_general() {
    let e = Env::new();
    let app = e.app();
    let keys = e.data.path().join("no-keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(
        keys.join("copilot-key-openai.txt"),
        "provider=openai\nkey=sk-x\nmodel=m\n",
    )
    .unwrap();
    let mut t = parse(&app.get_template_json("general"));
    t["id"] = json!("retro");
    app.save_template_json(&t.to_string());
    app.set_settings_json(r#"{"defaultTemplate": "retro"}"#);
    assert_eq!(app.plan(Job::Transcribe).unwrap().default_template, "retro");
    app.reset_template_json("retro");
    let s = parse(&app.get_settings_json());
    assert_eq!(
        s["settings"]["defaultTemplate"], "retro",
        "the stored choice is kept"
    );
    assert_eq!(s["effective"]["defaultTemplate"], "general");
    assert_eq!(
        app.plan(Job::Transcribe).unwrap().default_template,
        "general"
    );
}
