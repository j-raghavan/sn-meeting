//! F16-FR4 / F16-FR8: nativeGetRates and nativeSetRates through `App`: defaults with sources and
//! `asOf`, user edits in settings.json, validation, reset, and a bad hand-edited entry ignored.
use meeting::api::Core;
use meeting::app::App;
use meeting::rates::DEFAULTS_AS_OF;
use serde_json::{Value, json};
use std::fs;

struct Env {
    data: tempfile::TempDir,
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            data: tempfile::tempdir().unwrap(),
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn app(&self) -> App {
        let mut core = Core::device(self.data.path().to_str().unwrap());
        core.key_dir = self.data.path().join("no-keys");
        App::new(core, self.root.path().into())
    }
    fn saved(&self) -> Value {
        let s = fs::read_to_string(self.data.path().join("settings.json")).unwrap();
        serde_json::from_str(&s).unwrap()
    }
}

fn get(app: &App) -> Value {
    serde_json::from_str(&app.get_rates_json()).unwrap()
}

fn set(app: &App, v: Value) -> Value {
    serde_json::from_str(&app.set_rates_json(&v.to_string())).unwrap()
}

fn entry<'a>(v: &'a Value, model: &str) -> &'a Value {
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["model"] == model && e.get("validFrom").is_none())
        .unwrap()
}

#[test]
fn defaults_are_listed_with_their_date_and_source() {
    let e = Env::new();
    let v = get(&e.app());
    assert_eq!(v["asOf"], DEFAULTS_AS_OF);
    let w = entry(&v, "whisper-1");
    assert_eq!(w["unit"], "minute");
    assert_eq!(w["perMinute"], 0.006);
    assert_eq!(w["origin"], "default");
    assert!(w["source"].as_str().unwrap().starts_with("https://"));
    let g: Vec<&Value> = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["model"] == "gemini-3.8-flash")
        .collect();
    assert_eq!(g.len(), 2, "the 2027 price ships as its own entry");
    assert_eq!(g[1]["validFrom"], "2027-01-01");
    assert!(g[0]["note"].as_str().unwrap().contains("Check this rate"));
    assert_eq!(v["problems"], json!([]));
}

#[test]
fn an_edit_is_stored_dated_today_and_a_default_value_is_not() {
    let e = Env::new();
    let app = e.app();
    let mut list = get(&app)["entries"].clone();
    let w = list
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|x| x["model"] == "whisper-1")
        .unwrap();
    w["perMinute"] = json!(0.007);
    let v = set(&app, list.clone());
    let today = app.today().format("%Y-%m-%d").to_string();
    let w = entry(&v, "whisper-1");
    assert_eq!(
        (w["perMinute"].clone(), w["origin"].clone()),
        (json!(0.007), json!("user"))
    );
    assert_eq!(w["asOf"], today);
    assert_eq!(v["asOf"], today.clone().max(DEFAULTS_AS_OF.to_owned()));
    let saved = e.saved();
    assert_eq!(saved["rates"].as_array().unwrap().len(), 1, "only the edit");
    // Saving the same list again keeps the entry as it is.
    let again = set(&app, get(&app)["entries"].clone());
    assert_eq!(entry(&again, "whisper-1")["asOf"], today);
    assert_eq!(e.saved()["rates"], saved["rates"]);
    // A new rate for a model with no default.
    let v = set(
        &app,
        json!({"entries": [{"provider": "openai", "model": "gpt-9", "unit": "tokens",
                             "input": 1, "output": 2}]}),
    );
    assert_eq!(entry(&v, "gpt-9")["origin"], "user");
    assert_eq!(
        entry(&v, "whisper-1")["origin"],
        "default",
        "the list replaces"
    );
    // Reset to defaults.
    let v = set(&app, json!([]));
    assert_eq!(entry(&v, "whisper-1")["perMinute"], 0.006);
    assert!(e.saved().get("rates").is_none());
}

#[test]
fn a_bad_entry_is_refused_with_its_field_and_nothing_is_saved() {
    let e = Env::new();
    let app = e.app();
    let bad = json!([{"provider": "openai", "model": "whisper-1", "unit": "minute",
                      "perMinute": -2}]);
    let v = set(&app, bad);
    assert_eq!(v["field"], "rates[0].perMinute");
    assert!(!e.data.path().join("settings.json").exists());
    let v = set(&app, json!([{"provider": "nobody", "model": "m"}]));
    assert_eq!(v["field"], "rates[0]");
    assert_eq!(set(&app, json!({"x": 1}))["error"], "rates must be a list");
    assert_eq!(
        serde_json::from_str::<Value>(&app.set_rates_json("{")).unwrap()["error"],
        "rates must be JSON"
    );
}

#[test]
fn a_bad_hand_edited_entry_is_ignored_with_a_line() {
    let e = Env::new();
    let file = json!({"schema": 1, "rates": [
        {"provider": "openai", "model": "gpt-4o-mini", "unit": "tokens", "input": "cheap",
         "asOf": "2026-10-10", "source": "me"},
        {"provider": "openai", "model": "whisper-1", "unit": "minute", "perMinute": 0.01,
         "asOf": "2026-10-10", "source": "me"}]});
    fs::write(e.data.path().join("settings.json"), file.to_string()).unwrap();
    let v = get(&e.app());
    assert_eq!(
        v["problems"],
        json!(["Your rate for gpt-4o-mini has an error; using the default"])
    );
    assert_eq!(entry(&v, "gpt-4o-mini")["input"], 0.15);
    assert_eq!(entry(&v, "whisper-1")["perMinute"], 0.01);
}

#[test]
fn rates_are_not_changed_by_a_settings_patch() {
    let e = Env::new();
    let app = e.app();
    let patch = json!({"rates": [{"provider": "openai", "model": "x"}], "mic": "auto"});
    let v: Value = serde_json::from_str(&app.set_settings_json(&patch.to_string())).unwrap();
    assert!(v.get("error").is_none(), "{v}");
    assert!(e.saved().get("rates").is_none());
}

#[test]
fn a_newer_settings_file_is_not_written() {
    let e = Env::new();
    fs::write(e.data.path().join("settings.json"), r#"{"schema": 99}"#).unwrap();
    let v = set(&e.app(), json!([]));
    assert!(v["error"].as_str().unwrap().contains("newer version"));
}
