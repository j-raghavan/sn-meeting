//! F18-FR9 / F18-AC4 / EC27: a recording started during an event links to it by time; with
//! overlapping events the nearest start wins and the rest are alternatives. Transparent and
//! all-day events are skipped, and the setting turns it off.
mod libcommon;

use libcommon::cal::*;
use libcommon::*;
use serde_json::{Value, json};

fn event_of(dir: &std::path::Path, id: &str) -> Value {
    serde_json::to_value(read_sidecar(dir, id)).unwrap()["event"].clone()
}

#[test]
fn overlapping_events_link_to_the_nearest_start() {
    let env = Env::new();
    let app = env.app();
    let a = ev("ten", "Planning", TEN, TEN + 60 * MIN);
    let b = ev("half", "Weekly sync", TEN + 30 * MIN, TEN + 60 * MIN);
    calendar(&env, &app, vec![a.clone(), b.clone()]);
    let (id, dir) = record(&app, TEN + 32 * MIN);
    let e = event_of(&dir, &id);
    assert_eq!(e["eventKey"], b.event_key, "F18-AC4: the 10:30 event");
    assert_eq!(e["alternatives"], json!([a.event_key]));
    assert_eq!(e["linkSource"], "auto");
    assert_eq!(e["title"], "Weekly sync");
    assert_eq!(e["calendar"], "Work");
    assert_eq!(e["attendees"][0]["name"], "Priya Shah");
    assert_eq!(e["agenda"], "1. Budget");
    assert_eq!(e["seriesUid"], "half");
    let sc = read_sidecar(&dir, &id);
    assert_eq!(sc.title.as_deref(), Some("Weekly sync"));
    assert_eq!(sc.title_source, Some(meeting::sidecar::TitleSource::Event));
    let raw = std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
    assert!(
        raw.contains("\"titleSource\": \"event\"") || raw.contains("\"titleSource\":\"event\""),
        "{raw}"
    );
    assert!(!raw.contains("LINKSECRET"), "INV15");
}

#[test]
fn five_minutes_early_counts_and_skipped_kinds_never_link() {
    let env = Env::new();
    let app = env.app();
    let mut free = ev("free", "Focus time", TEN, TEN + 60 * MIN);
    free.transp = "transparent".into();
    let mut day = ev("day", "Offsite", TEN - 600 * MIN, TEN + 840 * MIN);
    day.all_day = true;
    let next = ev("next", "Review", TEN + 120 * MIN, TEN + 150 * MIN);
    calendar(&env, &app, vec![free, day, next.clone()]);
    let (id, dir) = record(&app, TEN + 10 * MIN);
    assert_eq!(
        event_of(&dir, &id),
        Value::Null,
        "transparent and all-day are skipped"
    );
    assert_eq!(read_sidecar(&dir, &id).title, None);
    let (id, dir) = record(&app, TEN + 115 * MIN);
    assert_eq!(
        event_of(&dir, &id)["eventKey"],
        next.event_key,
        "5 min early"
    );
    let (id, dir) = record(&app, TEN + 150 * MIN);
    assert_eq!(event_of(&dir, &id), Value::Null, "after its end");
}

#[test]
fn the_setting_turns_it_off_and_no_calendar_changes_nothing() {
    let env = Env::new();
    let app = env.app();
    let (id, dir) = record(&app, TEN);
    assert_eq!(event_of(&dir, &id), Value::Null, "no links file");
    calendar(&env, &app, vec![ev("ten", "Planning", TEN, TEN + 60 * MIN)]);
    let r = app.set_settings_json(r#"{"calendar":{"linkByTime":false}}"#);
    assert!(r.contains("\"linkByTime\":false"), "{r}");
    let (id, dir) = record(&app, TEN + 5 * MIN);
    assert_eq!(event_of(&dir, &id), Value::Null);
}
