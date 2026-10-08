//! F10-FR4 / F4-FR3 / F4-FR5: nativeGetSettings and nativeSetSettings through `App`. Every reply
//! and the saved file are checked for the fixture keys (INV2).
mod common;

use common::*;
use meeting::api::Core;
use meeting::app::App;
use serde_json::{Value, json};
use std::fs;

struct Env {
    keys: tempfile::TempDir,
    data: tempfile::TempDir,
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            keys: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn app(&self) -> App {
        let mut core = Core::device(self.data.path().to_str().unwrap());
        core.key_dir = self.keys.path().into();
        App::new(core, self.root.path().into())
    }
    fn key(&self, name: &str, body: &str) {
        fs::write(self.keys.path().join(name), body).unwrap();
    }
    fn saved(&self) -> String {
        fs::read_to_string(self.data.path().join("settings.json")).unwrap_or_default()
    }
}

fn get(app: &App) -> Value {
    let s = app.get_settings_json();
    assert_no_key(&s);
    serde_json::from_str(&s).unwrap()
}

fn set(app: &App, patch: Value) -> Value {
    let s = app.set_settings_json(&patch.to_string());
    assert_no_key(&s);
    serde_json::from_str(&s).unwrap()
}

fn openai_only(e: &Env) {
    e.key(
        "copilot-key-openai.txt",
        &format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\n"),
    );
}

#[test]
fn only_the_openai_key_selects_openai_for_both_roles() {
    // F10-AC1 / F4-AC3.
    let e = Env::new();
    openai_only(&e);
    let s = get(&e.app());
    assert_eq!(s["origin"], "defaults");
    assert_eq!(
        s["effective"]["transcribe"],
        json!({"provider": "openai", "model": "gpt-transcribe", "modelSource": "default"})
    );
    assert_eq!(
        s["effective"]["summarise"],
        json!({"provider": "openai", "model": "gpt-4o-mini", "modelSource": "keyFile"})
    );
    assert_eq!(
        s["keys"]["openai"],
        json!({"status": "found", "file": "copilot-key-openai.txt", "model": "gpt-4o-mini", "transcribeModel": null})
    );
    for p in ["gemini", "anthropic"] {
        assert_eq!(
            s["keys"][p],
            json!({"status": "missing", "file": format!("copilot-key-{p}.txt"),
                   "detail": format!("Add copilot-key-{p}.txt to MyStyle/SnCopilot")})
        );
    }
    assert_eq!(
        s["effective"]["saveFolderPath"],
        json!(e.root.path().join("EXPORT/sn-meeting"))
    );
    assert_eq!(s["settings"]["saveFolder"], "EXPORT/sn-meeting");
}

#[test]
fn the_default_is_the_first_provider_with_a_key() {
    let e = Env::new();
    // No keys at all: OpenAI, with the code default for transcribe and nothing to summarise with.
    let s = get(&e.app());
    assert_eq!(s["effective"]["transcribe"]["provider"], "openai");
    assert_eq!(
        s["effective"]["summarise"],
        json!({"provider": "openai", "model": null, "modelSource": null})
    );
    // Anthropic only: it summarises, but transcription stays on OpenAI (no audio on Anthropic).
    e.key(
        "copilot-key-claude.txt",
        &format!("provider=anthropic\nkey={AN}\nmodel=claude-b\n"),
    );
    let s = get(&e.app());
    assert_eq!(s["effective"]["transcribe"]["provider"], "openai");
    assert_eq!(s["effective"]["summarise"]["provider"], "anthropic");
    assert_eq!(s["keys"]["anthropic"]["file"], "copilot-key-claude.txt");
    // Gemini too: Gemini wins both, and its transcribe_model line is used.
    e.key(
        "copilot-key-google.txt",
        &format!("provider=gemini\nkey={GE}\nmodel=gemini-z\ntranscribe_model=gemini-t\n"),
    );
    let s = get(&e.app());
    assert_eq!(
        s["effective"]["transcribe"],
        json!({"provider": "gemini", "model": "gemini-t", "modelSource": "keyFile"})
    );
    assert_eq!(s["effective"]["summarise"]["provider"], "gemini");
}

#[test]
fn vault_invalid_and_unreadable_keys_are_reported() {
    let e = Env::new();
    e.key(".copilot-key.enc", "ciphertext");
    e.key(
        "copilot-key-gemini.txt",
        &format!("provider=gemini key={GE} model=m"),
    );
    let s = get(&e.app());
    assert_eq!(s["keys"]["openai"]["status"], "vault");
    assert_eq!(
        s["keys"]["openai"]["detail"],
        "Your keys are in SN-Copilot's encrypted vault; Meetings v1 needs the plaintext copilot-key-openai.txt"
    );
    assert_eq!(s["keys"]["gemini"]["status"], "invalid");
    assert_eq!(
        s["keys"]["gemini"]["detail"],
        "gemini key file: provider=\"<invalid>\" mismatches filename suffix (expected canonical \"gemini\")"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(e.keys.path(), fs::Permissions::from_mode(0o000)).unwrap();
        let s = get(&e.app());
        fs::set_permissions(e.keys.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            s["keys"]["anthropic"],
            json!({"status": "unreadable", "detail": "anthropic key file: key file unreadable: permission denied"})
        );
    }
}

#[test]
fn a_key_echoed_in_the_model_line_is_scrubbed() {
    let e = Env::new();
    e.key(
        "copilot-key-openai.txt",
        &format!("provider=openai\nkey={OA}\nmodel={OA}\n"),
    );
    let s = get(&e.app()); // get() asserts the key is absent
    assert_eq!(s["keys"]["openai"]["model"], "[redacted]");
}

#[test]
fn choosing_a_provider_needs_its_key() {
    let e = Env::new();
    openai_only(&e);
    let app = e.app();
    assert_eq!(
        set(&app, json!({"summarise": {"provider": "gemini"}})),
        json!({"error": "Add copilot-key-gemini.txt to MyStyle/SnCopilot", "field": "summarise.provider"})
    );
    assert_eq!(
        set(&app, json!({"transcribe": {"provider": "anthropic"}})),
        json!({"error": "anthropic cannot transcribe", "field": "transcribe.provider"})
    );
    assert_eq!(e.saved(), "", "a rejected patch writes nothing");
    let s = set(&app, json!({"transcribe": {"provider": "openai"}}));
    assert_eq!(s["settings"]["transcribe"]["provider"], "openai");
    assert_eq!(s["origin"], "file");
    // null clears back to "first provider with a key".
    let s = set(&app, json!({"transcribe": {"provider": null}}));
    assert_eq!(s["settings"]["transcribe"]["provider"], json!(null));
}

#[test]
fn a_stored_provider_whose_key_went_away_is_kept() {
    // The rule applies when the provider changes, not on every unrelated save.
    let e = Env::new();
    fs::write(
        e.data.path().join("settings.json"),
        r#"{"summarise": {"provider": "anthropic"}}"#,
    )
    .unwrap();
    let app = e.app();
    let s = set(&app, json!({"noticeShown": {"openai": true}}));
    assert_eq!(s["settings"]["summarise"]["provider"], "anthropic");
    assert_eq!(s["effective"]["summarise"]["provider"], "anthropic");
    assert_eq!(s["settings"]["noticeShown"]["openai"], true);
}

#[test]
fn model_overrides_win_and_clear() {
    let e = Env::new();
    openai_only(&e);
    let app = e.app();
    let s = set(&app, json!({"summarise": {"model": "  gpt-x  "}}));
    assert_eq!(
        s["effective"]["summarise"],
        json!({"provider": "openai", "model": "gpt-x", "modelSource": "override"})
    );
    for clear in [json!(null), json!(""), json!("   ")] {
        set(&app, json!({"summarise": {"model": "gpt-x"}}));
        let s = set(&app, json!({"summarise": {"model": clear}}));
        assert_eq!(s["settings"]["summarise"]["model"], json!(null), "{clear}");
        assert_eq!(s["effective"]["summarise"]["modelSource"], "keyFile");
    }
    for bad in ["two words", "tab\tmodel"] {
        assert_eq!(
            set(&app, json!({"transcribe": {"model": bad}})),
            json!({"error": "model name is invalid", "field": "transcribe.model"})
        );
    }
    let long = "m".repeat(129);
    assert_eq!(
        set(&app, json!({"summarise": {"model": long}}))["field"],
        "summarise.model"
    );
}

#[test]
fn a_model_override_is_cleared_when_its_provider_changes() {
    // S2: an OpenAI model name must not be sent to Gemini.
    let e = Env::new();
    openai_only(&e);
    e.key(
        "copilot-key-gemini.txt",
        &format!("provider=gemini\nkey={GE}\nmodel=gemini-3.8-flash\n"),
    );
    let app = e.app();
    set(
        &app,
        json!({"summarise": {"provider": "openai", "model": "gpt-x"}}),
    );
    let s = set(&app, json!({"summarise": {"provider": "gemini"}}));
    assert_eq!(s["settings"]["summarise"]["model"], json!(null), "{s}");
    assert_eq!(
        s["effective"]["summarise"],
        json!({"provider": "gemini", "model": "gemini-3.8-flash", "modelSource": "keyFile"})
    );
    // Clearing the provider back to automatic clears it too.
    set(
        &app,
        json!({"transcribe": {"provider": "openai", "model": "whisper-1"}}),
    );
    let s = set(&app, json!({"transcribe": {"provider": null}}));
    assert_eq!(s["settings"]["transcribe"]["model"], json!(null));
    // A model sent with the new provider is kept; an unrelated patch keeps both.
    let s = set(
        &app,
        json!({"transcribe": {"provider": "gemini", "model": "gemini-x"}}),
    );
    assert_eq!(s["settings"]["transcribe"]["model"], "gemini-x");
    let s = set(&app, json!({"mic": "auto"}));
    assert_eq!(s["settings"]["transcribe"]["model"], "gemini-x");
}

#[test]
fn core_owned_fields_and_bad_patches() {
    let e = Env::new();
    let app = e.app();
    let s = set(
        &app,
        json!({"schema": 9, "previousFolders": ["x"], "mic": "usb:USB Audio"}),
    );
    assert_eq!(s["settings"]["schema"], 1);
    assert_eq!(s["settings"]["previousFolders"], json!([]));
    assert_eq!(s["settings"]["mic"], "usb:USB Audio");
    assert_eq!(
        set(&app, json!([1])),
        json!({"error": "settings must be a JSON object", "field": null})
    );
    let v: Value = serde_json::from_str(&app.set_settings_json("{nope")).unwrap();
    assert_eq!(v, json!({"error": "settings must be JSON", "field": null}));
    let v = set(&app, json!({"debug": {"segmentMs": "soon"}}));
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with("invalid settings: "),
        "{v}"
    );
    assert_eq!(v["field"], json!(null));
}

#[test]
fn changes_persist_and_unknown_fields_survive() {
    let e = Env::new();
    openai_only(&e);
    fs::write(
        e.data.path().join("settings.json"),
        r#"{"future": {"a": 1}}"#,
    )
    .unwrap();
    let app = e.app();
    set(
        &app,
        json!({"transcribe": {"provider": "openai", "model": "whisper-z"}, "x": 2}),
    );
    assert_no_key(&e.saved());
    let reloaded = get(&e.app());
    assert_eq!(reloaded["origin"], "file");
    assert_eq!(reloaded["settings"]["transcribe"]["model"], "whisper-z");
    assert_eq!(reloaded["settings"]["future"], json!({"a": 1}));
    assert_eq!(reloaded["settings"]["x"], 2);
}

#[cfg(unix)]
#[test]
fn a_save_that_fails_changes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    let app = e.app();
    fs::set_permissions(e.data.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let v = set(&app, json!({"mic": "bt:Jabra"}));
    fs::set_permissions(e.data.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        v,
        json!({"error": "can't save settings: permission denied"})
    );
    assert_eq!(get(&app)["settings"]["mic"], "auto");
}

#[test]
fn key_files_with_every_fixture_key_never_leak_into_settings() {
    let e = Env::new();
    write_keys(e.keys.path());
    let app = e.app();
    get(&app);
    set(
        &app,
        json!({"summarise": {"provider": "anthropic", "model": "claude-b"}}),
    );
    assert_no_key(&e.saved());
}

#[test]
fn a_key_typed_as_model_override_is_refused() {
    // INV2: a key pasted into the model field is never stored or echoed.
    let e = Env::new();
    openai_only(&e);
    let app = e.app();
    for model in [OA.to_owned(), format!("x{OA}y")] {
        let r = set(&app, json!({"summarise": {"model": model}}));
        assert_eq!(
            r,
            json!({"error": "that is an API key, not a model name", "field": "summarise.model"})
        );
    }
    assert!(!e.saved().contains(OA));
    // A model that merely shares a few characters is fine.
    let r = set(&app, json!({"summarise": {"model": "sk-test"}}));
    assert_eq!(r["settings"]["summarise"]["model"], "sk-test");
}

#[test]
fn a_cut_key_typed_as_model_is_refused() {
    // INV2, scrub's rule: 12 or more of a key's first or last characters are key material.
    let e = Env::new();
    openai_only(&e);
    let app = e.app();
    let n = OA.chars().count();
    let head: String = OA.chars().take(12).collect();
    let tail: String = OA.chars().skip(n - 13).collect();
    for model in [head.clone(), format!("m-{tail}")] {
        let r = set(&app, json!({"transcribe": {"model": model}}));
        assert_eq!(
            r,
            json!({"error": "that is an API key, not a model name", "field": "transcribe.model"}),
            "{model}"
        );
    }
    assert!(!e.saved().contains(&head));
    // Eleven characters are not enough to be key material.
    let short: String = OA.chars().take(11).collect();
    let r = set(&app, json!({"transcribe": {"model": short.clone()}}));
    assert_eq!(r["settings"]["transcribe"]["model"], short);
}
