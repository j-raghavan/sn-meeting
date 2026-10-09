//! F18-FR8: the event sheet (details, the keyword-picked template, "Earlier in this series",
//! "From last time", the meetings linked to it), "● Record this meeting" linking whatever the
//! time, and the keyword rules in Settings.
mod libcommon;

use libcommon::cal::*;
use libcommon::*;
use serde_json::{Value, json};

fn parse_v(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn the_sheet_and_record_this_meeting() {
    let env = Env::new();
    let app = env.app();
    let mut daily = ev("daily", "Team Stand-up", TEN, TEN + 15 * MIN);
    daily.recurring = true;
    let mut before = ev(
        "daily",
        "Team Stand-up",
        TEN - 24 * 60 * MIN,
        TEN - 24 * 60 * MIN + 15 * MIN,
    );
    before.recurring = true;
    before.event_key = format!(
        "{}:daily:yesterday",
        daily.event_key.split(':').next().unwrap()
    );
    let later = ev(
        "later",
        "Quarterly review",
        TEN + 300 * MIN,
        TEN + 360 * MIN,
    );
    calendar(
        &env,
        &app,
        vec![before.clone(), daily.clone(), later.clone()],
    );
    let (old, _) = record(&app, before.start_ms + MIN);

    let s = parse_v(&app.event_json(&daily.event_key));
    assert_eq!(s["calendar"], "Work");
    assert_eq!(s["event"]["title"], "Team Stand-up");
    assert_eq!(s["event"]["agenda"], "1. Budget");
    assert_eq!(
        s["template"],
        json!({"id": "standup", "keyword": "stand-up"}),
        "case-insensitive"
    );
    assert_eq!(s["series"][0]["meetingId"], old.as_str());
    assert_eq!(s["linked"], json!([]));
    let q = parse_v(&app.event_json(&later.event_key));
    assert_eq!(
        q["template"],
        json!({"id": "general", "keyword": null}),
        "Settings' default"
    );
    assert_eq!(q["series"], json!([]), "not recurring");
    assert!(parse_v(&app.event_json("0123abcd:nope:-"))["error"].is_string());
    assert!(parse_v(&app.event_json("junk"))["error"].is_string());

    // "● Record this meeting" on the later event, hours early: linked to it (user), with the
    // template its keyword picks only when one matched.
    let b = parse_v(&app.begin_linked(
        TEN + MIN,
        &mic_json().to_string(),
        &audio_json().to_string(),
        Some(&later.event_key),
    ));
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1000, "");
    let m = parse_v(&app.get_meeting_json(&id));
    assert_eq!(m["meeting"]["event"]["eventKey"], later.event_key.as_str());
    assert_eq!(m["meeting"]["event"]["linkSource"], "user");
    assert_eq!(m["meeting"]["templateId"], Value::Null);
    assert_eq!(
        parse_v(&app.event_json(&later.event_key))["linked"],
        json!([id])
    );
    // By time during the stand-up: its keyword picks the template.
    let (sid, dir) = record(&app, TEN + 2 * MIN);
    assert_eq!(
        read_sidecar(&dir, &sid).template_id.as_deref(),
        Some("standup")
    );
    // A key no longer in the calendar records anyway, linked by time if anything qualifies.
    let b = parse_v(&app.begin_linked(
        TEN + 400 * MIN,
        &mic_json().to_string(),
        &audio_json().to_string(),
        Some("0123abcd:gone:-"),
    ));
    assert!(b["id"].is_string(), "{b}");
}

#[test]
fn keyword_rules_in_settings() {
    let env = Env::new();
    let app = env.app();
    let r = parse_v(&app.set_settings_json(
        r#"{"calendar":{"templateRules":[{"keyword":"  Sync ","templateId":"one-on-one"}]}}"#,
    ));
    assert_eq!(
        r["settings"]["calendar"]["templateRules"],
        json!([{"keyword": "Sync", "templateId": "one-on-one"}])
    );
    assert_eq!(
        app.template_for("weekly SYNC"),
        ("one-on-one".to_owned(), Some("Sync".to_owned()))
    );
    assert_eq!(app.template_for("Lunch").1, None);
    for bad in [
        r#"[{"keyword":"","templateId":"general"}]"#,
        r#"[{"keyword":"x","templateId":"Not An Id"}]"#,
    ] {
        let r = parse_v(
            &app.set_settings_json(&format!(r#"{{"calendar":{{"templateRules":{bad}}}}}"#)),
        );
        assert_eq!(r["field"], "calendar.templateRules", "{bad}: {r}");
    }
    let many: Vec<Value> = (0..51)
        .map(|i| json!({"keyword": format!("k{i}"), "templateId": "general"}))
        .collect();
    let r =
        parse_v(&app.set_settings_json(&json!({"calendar": {"templateRules": many}}).to_string()));
    assert_eq!(r["field"], "calendar.templateRules");
    // A link pasted as a keyword is refused, never stored (INV15).
    calendar(&env, &app, vec![]);
    let r = parse_v(
        &app.set_settings_json(
            &json!({"calendar": {"templateRules": [{"keyword": URL, "templateId": "general"}]}})
                .to_string(),
        ),
    );
    assert_eq!(r["error"], "that is a link or a key, not a keyword");
    // A rule for a template that's gone falls through to the default.
    app.set_settings_json(
        r#"{"calendar":{"templateRules":[{"keyword":"sync","templateId":"no-such"}]}}"#,
    );
    assert_eq!(app.template_for("sync").0, "general");
}

#[test]
fn hand_edited_rules_are_cleaned_on_load() {
    let env = Env::new();
    std::fs::write(
        env.data.path().join("settings.json"),
        r#"{"schema":1,"calendar":{"templateRules":[{"keyword":"ok","templateId":"general"},{"keyword":"","templateId":"general"},{"keyword":"x","templateId":"BAD"}]}}"#,
    )
    .unwrap();
    let app = env.app();
    let rules = app.settings().0.calendar.template_rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].keyword, "ok");
}
