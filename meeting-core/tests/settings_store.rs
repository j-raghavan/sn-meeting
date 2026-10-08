//! F10-FR7: settings.json is schema-versioned, written atomically, keeps unknown fields, and a
//! missing or corrupt file gives defaults (the bad one kept as settings.json.bad).
use meeting::api::Core;
use meeting::app::App;
use meeting::settings::{self, Origin, Settings};
use serde_json::{Value, json};
use std::fs;

fn defaults_json() -> Value {
    json!({
        "schema": 1, "mic": "auto", "saveFolder": "EXPORT/sn-meeting", "previousFolders": [],
        "transcribe": {"provider": null, "model": null},
        "summarise": {"provider": null, "model": null},
        "defaultTemplate": "general",
        "noticeShown": {"openai": false, "gemini": false, "anthropic": false},
        "debug": {"segmentMs": null}
    })
}

#[test]
fn the_defaults_have_the_documented_shape() {
    assert_eq!(
        serde_json::to_value(Settings::default()).unwrap(),
        defaults_json()
    );
}

#[test]
fn a_missing_file_gives_defaults_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    assert_eq!(load(&p), (Settings::default(), Origin::Defaults));
    assert!(!p.exists());
}

fn load(p: &std::path::Path) -> (Settings, Origin) {
    settings::load(p)
}

#[test]
fn unknown_fields_survive_a_save_at_every_level() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    let v = json!({
        "schema": 1, "mic": "bt:Jabra", "saveFolder": "Document/Meetings",
        "previousFolders": ["EXPORT/sn-meeting"],
        "transcribe": {"provider": "gemini", "model": null, "x": 1},
        "summarise": {"provider": null, "model": "m", "temperature": 0.2},
        "defaultTemplate": "general",
        "noticeShown": {"openai": true, "gemini": false, "anthropic": false, "deepseek": true},
        "debug": {"segmentMs": 60000, "y": [2]},
        "transcriptOptions": {"z": 3}
    });
    fs::write(&p, v.to_string()).unwrap();
    let (s, origin) = load(&p);
    assert_eq!(origin, Origin::File);
    assert_eq!(
        s.transcribe.provider,
        Some(meeting::keyfile::Provider::Gemini)
    );
    settings::save(&p, &s).unwrap();
    let back: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    assert_eq!(back, v);
}

#[test]
fn a_partial_file_fills_in_defaults() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(&p, r#"{"saveFolder": "Document/Meetings"}"#).unwrap();
    let (s, origin) = load(&p);
    assert_eq!(origin, Origin::File);
    assert_eq!(
        s,
        Settings {
            save_folder: "Document/Meetings".into(),
            ..Settings::default()
        }
    );
}

#[test]
fn corrupt_files_are_kept_as_bad() {
    let big = format!("{{\"mic\": \"{}\"}}", "x".repeat(300 * 1024));
    for body in ["{nope", "[1, 2]", r#"{"schema": "one"}"#, big.as_str()] {
        let d = tempfile::tempdir().unwrap();
        let p = settings::path_in(d.path());
        fs::write(&p, body).unwrap();
        assert_eq!(
            load(&p),
            (Settings::default(), Origin::Recovered),
            "{:.40}",
            body
        );
        assert!(!p.exists());
        assert_eq!(
            fs::read_to_string(d.path().join("settings.json.bad")).unwrap(),
            body
        );
        // The next save writes a fresh file, which then loads normally.
        settings::save(&p, &Settings::default()).unwrap();
        assert_eq!(load(&p).1, Origin::File);
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_recovered_too() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(&p, "{}").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(load(&p).1, Origin::Recovered);
    let bad = d.path().join("settings.json.bad");
    fs::set_permissions(&bad, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn a_stray_tmp_from_an_interrupted_save_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    let s = Settings {
        mic: meeting::settings::MicChoice::Usb("USB Audio".into()),
        ..Settings::default()
    };
    settings::save(&p, &s).unwrap();
    fs::write(d.path().join("settings.json.tmp"), "{trunc").unwrap();
    assert_eq!(load(&p), (s, Origin::File));
}

#[test]
fn the_app_loads_settings_from_its_data_dir() {
    let d = tempfile::tempdir().unwrap();
    let core = Core::device(d.path().to_str().unwrap());
    let app = App::new(core.clone(), d.path().into());
    assert_eq!(app.settings(), (Settings::default(), Origin::Defaults));
    let s = Settings {
        default_template: "standup".into(),
        ..Settings::default()
    };
    settings::save(&settings::path_in(d.path()), &s).unwrap();
    assert_eq!(
        App::new(core, d.path().into()).settings(),
        (s, Origin::File)
    );
    let dev = App::device("/data/user/0/dev.jraghavan.snmeeting/files");
    assert_eq!(
        dev.storage_root,
        std::path::PathBuf::from("/storage/emulated/0")
    );
    assert_eq!(dev.settings().1, Origin::Defaults);
}
