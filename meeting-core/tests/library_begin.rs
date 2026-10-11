//! F3-FR1: begin creates a `recording` sidecar with a unique id in the current save folder.
mod libcommon;

use libcommon::*;
use meeting::sidecar::MicKind;
use meeting::status::Status;
use serde_json::json;
use std::fs;

#[test]
fn begin_writes_a_recording_sidecar_and_returns_the_plan() {
    let e = Env::new();
    let app = e.app();
    let b = begin(&app, T0);
    let first = e.export().join(format!("{ID0}-01.m4a"));
    assert_eq!(
        b,
        json!({"id": ID0, "firstSegmentPath": first,
               "plan": {"maxDurationMs": 900_000, "maxFileBytes": 13_499_997}})
    );
    assert!(!first.exists(), "the shell records the audio, not the core");
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!(sc.status, Status::Recording);
    assert_eq!(sc.started_at, "2026-10-07T21:17:06-07:00");
    assert_eq!(sc.mic.as_ref().unwrap().kind, MicKind::Usb);
    assert_eq!(sc.audio.as_ref().unwrap().bitrate_bps, 96_000);
    assert_eq!(sc.template_id, None);
    let seg = serde_json::to_value(&sc.segments).unwrap();
    assert_eq!(
        seg,
        json!([{"file": format!("{ID0}-01.m4a"), "startOffsetMs": 0, "durationMs": null, "text": null}])
    );
    assert_eq!(app.liveness().recording.as_deref(), Some(ID0));
}

#[test]
fn a_second_meeting_in_the_same_second_gets_a_suffix() {
    // F3-AC5, at the library level (the app allows one recording at a time).
    let e = Env::new();
    let z = tz(PDT);
    let dir = e.folder("EXPORT/sn-meeting");
    let lib = library(&[&dir], &z);
    let a = lib.begin(T0, mic(), audio(), None).unwrap();
    let b = lib.begin(T0 + 400, mic(), audio(), None).unwrap();
    assert_eq!(
        (a.id.as_str(), b.id.as_str()),
        (ID0, "meeting-20261007-211706b")
    );
    assert!(dir.join("meeting-20261007-211706b.json").is_file());
}

#[test]
fn legacy_audio_in_a_previous_folder_takes_the_id_too() {
    let e = Env::new();
    let z = tz(PDT);
    let cur = e.folder("Document/Meetings");
    let old = e.folder("EXPORT/sn-meeting");
    fs::write(old.join(format!("{ID0}.m4a")), b"spike").unwrap();
    fs::write(cur.join("meeting-20261007-211706b-01.m4a"), b"orphan").unwrap();
    let lib = library(&[&cur, &old], &z);
    assert_eq!(
        lib.begin(T0, mic(), audio(), None).unwrap().id,
        "meeting-20261007-211706c"
    );
    // Unrelated names in the folders are not meetings.
    for other in [
        "notes.txt",
        "meeting-20261007-211706d.json.tmp",
        "meeting-20261007-211706d.transcript.md",
    ] {
        fs::write(cur.join(other), b"x").unwrap();
    }
    assert_eq!(
        lib.begin(T0, mic(), audio(), None).unwrap().id,
        "meeting-20261007-211706d"
    );
}

#[test]
fn twenty_six_in_one_second_is_the_limit() {
    let e = Env::new();
    let z = tz(PDT);
    let dir = e.folder("EXPORT/sn-meeting");
    let lib = library(&[&dir], &z);
    for _ in 0..26 {
        lib.begin(T0, mic(), audio(), None).unwrap();
    }
    assert_eq!(
        lib.begin(T0, mic(), audio(), None).unwrap_err().to_string(),
        "too many meetings started in one second"
    );
}

#[test]
fn the_id_and_start_use_the_device_offset() {
    let e = Env::new();
    let app = e.app_tz(5 * 3600 + 1800); // +05:30
    let b = begin(&app, T0);
    assert_eq!(b["id"], "meeting-20261008-094706");
    let sc = read_sidecar(&e.export(), "meeting-20261008-094706");
    assert_eq!(sc.started_at, "2026-10-08T09:47:06+05:30");
    let utc = Env::new();
    assert_eq!(begin(&utc.app_tz(0), T0)["id"], "meeting-20261008-041706");
}

#[test]
fn the_app_refuses_a_second_recording() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        begin(&app, T0 + 5_000),
        json!({"error": format!("already recording {ID0}")})
    );
}

#[test]
fn the_debug_segment_length_comes_from_settings() {
    let e = Env::new();
    let app = e.app();
    app.set_settings_json(r#"{"debug": {"segmentMs": 60000}}"#);
    assert_eq!(begin(&app, T0)["plan"]["maxDurationMs"], 60_000);
}

#[test]
fn the_meeting_goes_to_the_current_folder() {
    let e = Env::new();
    let app = e.app();
    let r = parse(&app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#));
    assert_eq!(r["settings"]["saveFolder"], "Document/Meetings");
    let b = begin(&app, T0);
    assert!(
        b["firstSegmentPath"]
            .as_str()
            .unwrap()
            .starts_with(e.root.path().join("Document/Meetings").to_str().unwrap())
    );
}

#[test]
fn bad_input_is_refused_and_writes_nothing() {
    let e = Env::new();
    let app = e.app();
    let audio = audio_json();
    let mic = mic_json();
    let cases = [
        (
            T0,
            json!({"kind": "wifi", "name": "x"}),
            audio.clone(),
            "micJson: ",
        ),
        (
            T0,
            json!({"kind": "usb", "name": "  "}),
            audio.clone(),
            "mic name is invalid",
        ),
        (
            T0,
            json!({"kind": "usb", "name": "a\u{7}b"}),
            audio.clone(),
            "mic name is invalid",
        ),
        (T0, mic.clone(), json!({"codec": "aac"}), "audioJson: "),
        (
            T0,
            mic.clone(),
            json!({"codec": "", "sampleRateHz": 1, "bitrateBps": 1, "channels": 1}),
            "codec is invalid",
        ),
        (
            T0,
            mic.clone(),
            json!({"codec": "a/b", "sampleRateHz": 1, "bitrateBps": 1, "channels": 1}),
            "codec is invalid",
        ),
        (
            T0,
            mic.clone(),
            json!({"codec": "aac-lc", "sampleRateHz": 0, "bitrateBps": 1, "channels": 1}),
            "sampleRateHz must be > 0",
        ),
        (
            T0,
            mic.clone(),
            json!({"codec": "aac-lc", "sampleRateHz": 1, "bitrateBps": 0, "channels": 1}),
            "bitrateBps must be > 0",
        ),
        (
            T0,
            mic.clone(),
            json!({"codec": "aac-lc", "sampleRateHz": 1, "bitrateBps": 1, "channels": 3}),
            "channels must be 1 or 2",
        ),
        (
            i64::MAX,
            mic.clone(),
            audio.clone(),
            "startedAtMs is out of range",
        ),
        (
            -1,
            mic.clone(),
            audio.clone(),
            "startedAtMs is out of range",
        ),
    ];
    for (at, m, a, want) in cases {
        let r = parse(&app.begin_json(at, &m.to_string(), &a.to_string()));
        let msg = r["error"].as_str().unwrap_or_else(|| panic!("{r}"));
        assert!(msg.starts_with(want), "{msg} vs {want}");
    }
    assert!(!e.export().exists() || fs::read_dir(e.export()).unwrap().next().is_none());
    assert_eq!(app.liveness().recording, None);
}

#[cfg(unix)]
#[test]
fn an_unwritable_folder_is_a_storage_error() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    let dir = e.folder("EXPORT/sn-meeting");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let r = begin(&e.app(), T0);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(r, json!({"error": "storage: permission denied"}));
}

#[test]
fn a_built_in_mic_is_recorded_as_such() {
    // F22-FR1: the sidecar names the device's own mic, and an older reader's kinds are unchanged.
    let e = Env::new();
    let app = e.app();
    let mic = json!({"kind": "builtin", "name": "Built-In Mic"});
    parse(&app.begin_json(T0, &mic.to_string(), &audio_json().to_string()));
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!(sc.mic.as_ref().unwrap().kind, MicKind::Builtin);
    assert_eq!(serde_json::to_value(MicKind::Usb).unwrap(), json!("usb"));
    assert_eq!(
        serde_json::to_value(MicKind::Bluetooth).unwrap(),
        json!("bluetooth")
    );
}
