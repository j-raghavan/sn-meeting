//! F10-FR6: the default template for new transcriptions is stored as a template id.
use meeting::api::Core;
use meeting::app::App;
use meeting::settings::{self, Origin, is_template_id};
use serde_json::{Value, json};
use std::fs;

#[test]
fn template_ids_are_kebab_case() {
    for ok in [
        "general",
        "standup",
        "one-on-one",
        "q4-review-2026",
        "a",
        &"x".repeat(64),
    ] {
        assert!(is_template_id(ok), "{ok}");
    }
    for bad in [
        "",
        "General",
        "one_on_one",
        "-lead",
        "trail-",
        "dou--ble",
        "../general",
        "a/b",
        "with space",
        "café",
        &"x".repeat(65),
    ] {
        assert!(!is_template_id(bad), "{bad:?}");
    }
}

fn set(app: &App, patch: Value) -> Value {
    serde_json::from_str(&app.set_settings_json(&patch.to_string())).unwrap()
}

#[test]
fn chosen_through_settings_with_null_back_to_general() {
    let d = tempfile::tempdir().unwrap();
    let mut core = Core::device(d.path().to_str().unwrap());
    core.key_dir = d.path().join("no-keys");
    let app = App::new(core, d.path().into());
    assert_eq!(
        set(&app, json!({"defaultTemplate": "standup"}))["settings"]["defaultTemplate"],
        "standup"
    );
    for bad in [json!("../etc"), json!("Standup"), json!(1)] {
        assert_eq!(
            set(&app, json!({"defaultTemplate": bad})),
            json!({"error": "invalid template id", "field": "defaultTemplate"})
        );
    }
    assert_eq!(
        set(&app, json!({"defaultTemplate": null}))["settings"]["defaultTemplate"],
        "general"
    );
}

#[test]
fn a_bad_id_in_the_file_reads_as_general() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(&p, r#"{"defaultTemplate": "../../x", "mic": "bt:Jabra"}"#).unwrap();
    let (s, origin) = settings::load(&p);
    assert_eq!(
        (s.default_template.as_str(), origin),
        ("general", Origin::File)
    );
    assert_eq!(String::from(s.mic), "bt:Jabra");
}
