//! F10-FR2 / F2-FR8: the remembered mic, by device name, in one place for home and Settings.
use meeting::api::Core;
use meeting::app::App;
use meeting::settings::{self, MicChoice, Origin};
use serde_json::{Value, json};
use std::fs;

#[test]
fn the_three_forms_round_trip() {
    for (text, mic) in [
        ("auto", MicChoice::Auto),
        (
            "usb:USB-Audio - USB Audio",
            MicChoice::Usb("USB-Audio - USB Audio".into()),
        ),
        (
            "bt:Jabra Evolve2 65",
            MicChoice::Bluetooth("Jabra Evolve2 65".into()),
        ),
        (
            "bt:Name: with colon",
            MicChoice::Bluetooth("Name: with colon".into()),
        ),
    ] {
        assert_eq!(
            MicChoice::try_from(text.to_owned()),
            Ok(mic.clone()),
            "{text}"
        );
        assert_eq!(String::from(mic), text);
    }
    assert_eq!(
        MicChoice::try_from("usb:  Padded  ".to_owned()),
        Ok(MicChoice::Usb("Padded".into()))
    );
    assert_eq!(MicChoice::default(), MicChoice::Auto);
}

#[test]
fn bad_forms_are_rejected() {
    for bad in ["", "AUTO", "usb", "usb:", "bt:   ", "wifi:x", "bt:a\nb"] {
        assert!(MicChoice::try_from(bad.to_owned()).is_err(), "{bad:?}");
    }
    let long = format!("usb:{}", "n".repeat(201));
    assert_eq!(MicChoice::try_from(long), Err("mic name is invalid".into()));
    assert!(MicChoice::try_from(format!("usb:{}", "é".repeat(200))).is_ok());
}

fn app(data: &std::path::Path) -> App {
    let mut core = Core::device(data.to_str().unwrap());
    core.key_dir = data.join("no-keys");
    App::new(core, data.into())
}

fn set(app: &App, patch: Value) -> Value {
    serde_json::from_str(&app.set_settings_json(&patch.to_string())).unwrap()
}

#[test]
fn set_through_settings_and_persisted() {
    let d = tempfile::tempdir().unwrap();
    let a = app(d.path());
    assert_eq!(
        set(&a, json!({"mic": "bt:Jabra"}))["settings"]["mic"],
        "bt:Jabra"
    );
    assert_eq!(
        app(d.path()).settings().0.mic,
        MicChoice::Bluetooth("Jabra".into())
    );
    assert_eq!(
        set(&a, json!({"mic": "usb:"})),
        json!({"error": "mic name is invalid", "field": "mic"})
    );
    assert_eq!(
        set(&a, json!({"mic": "headset"})),
        json!({"error": "mic must be auto, usb:<name> or bt:<name>", "field": "mic"})
    );
    assert_eq!(
        set(&a, json!({"mic": 3})),
        json!({"error": "mic must be a string", "field": "mic"})
    );
    // null resets to the default.
    assert_eq!(set(&a, json!({"mic": null}))["settings"]["mic"], "auto");
}

#[test]
fn a_bad_mic_in_the_file_reads_as_auto_and_keeps_the_rest() {
    for bad in [json!("bogus"), json!(7), json!("usb:")] {
        let d = tempfile::tempdir().unwrap();
        let p = settings::path_in(d.path());
        fs::write(
            &p,
            json!({"mic": bad, "saveFolder": "Document/Meetings"}).to_string(),
        )
        .unwrap();
        let (s, origin) = settings::load(&p);
        assert_eq!(origin, Origin::File, "{bad}");
        assert_eq!(
            (s.mic, s.save_folder.as_str()),
            (MicChoice::Auto, "Document/Meetings")
        );
    }
}
