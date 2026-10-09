//! F14-FR8: the updater's choices live in settings.json's `updates` object: auto-check (on by
//! default, as inkread), opt-in auto-install, the last answered check and a skipped version.
//! Set through nativeSetSettings like every other setting; unknown fields are kept.
use meeting::api::Core;
use meeting::app::App;
use meeting::settings::{self, Origin, Settings};
use serde_json::{Value, json};
use std::fs;

fn app(d: &tempfile::TempDir) -> App {
    let mut core = Core::device(d.path().to_str().unwrap());
    core.key_dir = d.path().join("no-keys");
    App::new(core, d.path().into())
}

fn set(app: &App, patch: Value) -> Value {
    serde_json::from_str(&app.set_settings_json(&patch.to_string())).unwrap()
}

#[test]
fn defaults_check_automatically_and_install_only_when_asked() {
    let u = serde_json::to_value(Settings::default()).unwrap()["updates"].clone();
    assert_eq!(
        u,
        json!({"autoCheck": true, "autoInstall": false, "lastCheckMs": 0, "skipVersion": ""})
    );
}

#[test]
fn updates_are_set_by_patch_and_saved() {
    let d = tempfile::tempdir().unwrap();
    let a = app(&d);
    let r = set(
        &a,
        json!({"updates": {"lastCheckMs": 1_791_000_000_000u64}}),
    );
    assert_eq!(
        r["settings"]["updates"]["lastCheckMs"],
        1_791_000_000_000u64
    );
    assert_eq!(
        r["settings"]["updates"]["autoCheck"], true,
        "a patch merges"
    );
    let r = set(
        &a,
        json!({"updates": {"skipVersion": "0.2.0", "autoInstall": true}}),
    );
    assert_eq!(r["settings"]["updates"]["skipVersion"], "0.2.0");
    let (s, origin) = settings::load(&settings::path_in(d.path()));
    assert_eq!(origin, Origin::File);
    assert!(s.updates.auto_install);
    assert_eq!(s.updates.last_check_ms, 1_791_000_000_000);
    assert_eq!(s.updates.skip_version, "0.2.0");
}

#[test]
fn a_bad_update_patch_is_refused_by_field() {
    let d = tempfile::tempdir().unwrap();
    let a = app(&d);
    let r = set(&a, json!({"updates": {"skipVersion": "x".repeat(65)}}));
    assert_eq!(r["field"], "updates.skipVersion");
    let r = set(&a, json!({"updates": {"skipVersion": "0.2.0\n"}}));
    assert_eq!(r["field"], "updates.skipVersion");
    let r = set(&a, json!({"updates": {"autoCheck": "yes"}}));
    assert!(r["error"].is_string(), "{r}");
    let r = set(&a, json!({"updates": {"lastCheckMs": -1}}));
    assert!(r["error"].is_string(), "{r}");
    assert!(!settings::path_in(d.path()).exists(), "nothing was written");
}

#[test]
fn unknown_update_fields_survive_and_a_bad_value_reads_as_its_default() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    let v = json!({
        "schema": 1,
        "saveFolder": "Document/Meetings",
        "updates": {"autoCheck": false, "lastCheckMs": "soon", "skipVersion": "\u{7}", "channel": "beta"}
    });
    fs::write(&p, v.to_string()).unwrap();
    let (s, origin) = settings::load(&p);
    assert_eq!(origin, Origin::File);
    assert!(!s.updates.auto_check, "the good field is kept");
    assert_eq!(
        s.updates.last_check_ms, 0,
        "a wrong type reads as the default"
    );
    assert_eq!(s.updates.skip_version, "", "a control character is dropped");
    assert_eq!(s.save_folder, "Document/Meetings");
    let a = app(&d);
    set(&a, json!({"updates": {"lastCheckMs": 5}}));
    let saved: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(saved["updates"]["channel"], "beta");
    assert_eq!(saved["updates"]["lastCheckMs"], 5);
}
