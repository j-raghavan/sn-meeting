//! F10-FR7: settings.json is schema-versioned, written atomically, keeps unknown fields, and a
//! missing or corrupt file gives defaults (the bad one kept as settings.json.<time>.bad).
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
        "debug": {"segmentMs": null},
        "updates": {"autoCheck": true, "autoInstall": false, "lastCheckMs": 0, "skipVersion": ""},
        "calendar": {"refreshMin": 30, "linkByTime": true, "eventInMinutes": true}
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
        "updates": {"autoCheck": false, "autoInstall": false, "lastCheckMs": 7, "skipVersion": "0.2.0", "w": 4},
        "calendar": {"refreshMin": 60, "linkByTime": false, "eventInMinutes": false, "v": 5},
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
        assert_eq!(fs::read_to_string(bad_file(d.path())).unwrap(), body);
        // The next save writes a fresh file, which then loads normally.
        settings::save(&p, &Settings::default()).unwrap();
        assert_eq!(load(&p).1, Origin::File);
    }
}

fn no_bad_file(dir: &std::path::Path) -> bool {
    fs::read_dir(dir)
        .unwrap()
        .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".bad"))
}

/// The one `settings.json.<time>.bad` in `dir`.
fn bad_file(dir: &std::path::Path) -> std::path::PathBuf {
    let bad: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("settings.json.") && n.ends_with(".bad")
        })
        .collect();
    assert_eq!(bad.len(), 1, "{bad:?}");
    bad[0].clone()
}

#[test]
fn each_corrupt_file_is_kept_under_its_own_name() {
    // N3: a second corrupt file doesn't overwrite the first one kept.
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    for body in ["{one", "{two"] {
        fs::write(&p, body).unwrap();
        assert_eq!(load(&p).1, Origin::Recovered);
    }
    let mut kept: Vec<String> = fs::read_dir(d.path())
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    kept.sort();
    assert_eq!(kept, ["{one", "{two"]);
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_left_where_it_is() {
    // N3: a read error may be passing (storage not ready): the file is not moved aside.
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(&p, r#"{"saveFolder": "Notes"}"#).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(load(&p), (Settings::default(), Origin::Unreadable));
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        fs::read_dir(d.path()).unwrap().count(),
        1,
        "nothing moved or added"
    );
    assert_eq!(load(&p).0.save_folder, "Notes");
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

#[test]
fn invalid_folder_in_file_reads_as_default() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(
        &p,
        r#"{"saveFolder": "../../../etc", "previousFolders": ["Document/Meetings", "/abs",
            "MyStyle/SnCopilot", "a/../b", "", "Notes"],
            "transcribe": {"model": "has space"}, "summarise": {"model": "gpt-x"}}"#,
    )
    .unwrap();
    let (s, origin) = load(&p);
    assert_eq!(origin, Origin::File);
    assert_eq!(s.save_folder, "EXPORT/sn-meeting");
    assert_eq!(s.previous_folders, ["Document/Meetings", "Notes"]);
    assert_eq!(s.transcribe.model, None);
    assert_eq!(s.summarise.model.as_deref(), Some("gpt-x"));
    let core = Core::device(d.path().to_str().unwrap());
    let app = App::new(core, "/storage/emulated/0".into());
    let v: serde_json::Value = serde_json::from_str(&app.get_settings_json()).unwrap();
    assert_eq!(
        v["effective"]["saveFolderPath"],
        "/storage/emulated/0/EXPORT/sn-meeting"
    );
}

#[test]
fn bad_role_provider_in_file_does_not_block_other_patches() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    fs::write(&p, r#"{"transcribe": {"provider": "anthropic"}}"#).unwrap();
    assert_eq!(
        load(&p).0.transcribe.provider,
        None,
        "anthropic can't transcribe"
    );
    let mut core = Core::device(d.path().to_str().unwrap());
    core.key_dir = d.path().join("no-keys");
    let app = App::new(core, d.path().into());
    let r: serde_json::Value =
        serde_json::from_str(&app.set_settings_json(r#"{"mic": "auto"}"#)).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    // A stored value is only re-checked when its role changes.
    let current = Settings {
        transcribe: serde_json::from_value(
            serde_json::json!({"provider": "anthropic", "model": "x y"}),
        )
        .unwrap(),
        ..Settings::default()
    };
    let ks = |p| settings::key_status(&d.path().join("no-keys"), p);
    let ctx = settings::Ctx {
        storage_root: d.path(),
        key_status: &ks,
        secrets: &[],
    };
    let next = settings::apply(&current, &serde_json::json!({"mic": "auto"}), &ctx).unwrap();
    assert_eq!(next.transcribe, current.transcribe);
    let e = settings::apply(
        &current,
        &serde_json::json!({"transcribe": {"model": "y z"}}),
        &ctx,
    );
    assert_eq!(e.unwrap_err().field, Some("transcribe.model"));
}

#[test]
fn newer_schema_is_not_silently_rewritten() {
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    let newer = r#"{"schema": 2, "saveFolder": "Document/Meetings", "brandNew": {"x": 1}}"#;
    fs::write(&p, newer).unwrap();
    assert_eq!(load(&p), (Settings::default(), Origin::Newer));
    let mut core = Core::device(d.path().to_str().unwrap());
    core.key_dir = d.path().join("no-keys");
    let app = App::new(core, d.path().into());
    let v: Value = serde_json::from_str(&app.get_settings_json()).unwrap();
    assert_eq!(v["origin"], "newer");
    let r: Value = serde_json::from_str(&app.set_settings_json(r#"{"mic": "auto"}"#)).unwrap();
    assert!(
        r["error"].as_str().unwrap().contains("newer version"),
        "{r}"
    );
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        newer,
        "the file is untouched"
    );
    assert!(no_bad_file(d.path()));
    // schema 1 and a missing schema still load.
    fs::write(&p, r#"{"saveFolder": "Notes"}"#).unwrap();
    assert_eq!(load(&p).1, Origin::File);
}

#[test]
fn a_wrongly_typed_field_reads_as_its_default_and_keeps_the_folders() {
    // A hand edit with one bad value must not reset everything: losing saveFolder and
    // previousFolders would make meetings vanish from Home.
    let d = tempfile::tempdir().unwrap();
    let p = settings::path_in(d.path());
    let v = json!({
        "schema": 1, "mic": "bt:Jabra", "saveFolder": "Document/Meetings",
        "previousFolders": ["EXPORT/sn-meeting"],
        "transcribe": {"provider": "gemini", "model": 42},
        "summarise": "anthropic",
        "defaultTemplate": "standup",
        "noticeShown": {"openai": "yes", "gemini": true},
        "debug": {"segmentMs": -5},
        "extra": {"kept": true}
    });
    fs::write(&p, v.to_string()).unwrap();
    let (s, origin) = load(&p);
    assert_eq!(origin, Origin::File);
    assert_eq!(s.save_folder, "Document/Meetings");
    assert_eq!(s.previous_folders, ["EXPORT/sn-meeting"]);
    assert_eq!(s.mic, settings::MicChoice::Bluetooth("Jabra".into()));
    assert_eq!(
        s.transcribe.provider,
        Some(meeting::keyfile::Provider::Gemini)
    );
    assert_eq!(s.transcribe.model, None);
    assert_eq!(s.summarise, Settings::default().summarise);
    assert_eq!(s.default_template, "standup");
    assert!(!s.notice_shown.openai);
    assert!(s.notice_shown.gemini);
    assert_eq!(s.debug.segment_ms, None);
    assert!(no_bad_file(d.path()));
    // And the other way round: the folders themselves wrongly typed read as their defaults.
    let v = json!({"schema": 1, "saveFolder": 7, "previousFolders": "EXPORT", "defaultTemplate": "lecture"});
    fs::write(&p, v.to_string()).unwrap();
    let (s, origin) = load(&p);
    assert_eq!(origin, Origin::File);
    assert_eq!(s.save_folder, "EXPORT/sn-meeting");
    assert!(s.previous_folders.is_empty());
    assert_eq!(s.default_template, "lecture");
}
