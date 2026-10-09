//! F18-FR9 / F18-AC4 / EC27: a recording started during an event links to it by time; with
//! overlapping events the nearest start wins and the rest are alternatives. Transparent and
//! all-day events are skipped, and the setting turns it off.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::calendar::cache::{self, CacheFile, LinkStatus};
use meeting::calendar::ics::{Attendee, Event};
use serde_json::{Value, json};
use std::path::PathBuf;

/// 2026-10-09 10:00 PDT.
pub const TEN: i64 = 1_791_565_200_000;
pub const MIN: i64 = 60_000;
pub const URL: &str = "https://calendar.google.com/calendar/ical/me%40example.com/private-LINKSECRET0123456789/basic.ics";

pub fn ev(uid: &str, title: &str, start: i64, end: i64) -> Event {
    let rfc = |ms| meeting::tz::rfc3339(&meeting::tz::local_at(&tz(PDT), ms).unwrap());
    Event {
        event_key: format!("{}:{uid}:-", meeting::calendar::links::link_id(URL)),
        uid: uid.into(),
        recurrence_id: None,
        series_uid: uid.into(),
        recurring: false,
        title: title.into(),
        start: rfc(start),
        end: rfc(end),
        start_ms: start,
        end_ms: end,
        all_day: false,
        location: Some("Room 4".into()),
        attendees: vec![Attendee {
            name: Some("Priya Shah".into()),
            email: Some("priya@example.com".into()),
        }],
        organizer: None,
        agenda: Some("1. Budget".into()),
        status: Some("confirmed".into()),
        transp: "opaque".into(),
        tz_unknown: false,
    }
}

/// The links file with [URL] and a cache holding `events`, as a refresh would leave them.
pub fn calendar(env: &Env, app: &App, events: Vec<Event>) {
    let p = env.root.path().join("MyStyle/Meetings/calendar-links.txt");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, format!("{URL}\n")).unwrap();
    let c = CacheFile {
        link_id: meeting::calendar::links::link_id(URL),
        calendar: Some("Work".into()),
        fetched_at_ms: TEN,
        attempted_at_ms: TEN,
        status: LinkStatus::Ok,
        events,
        ..CacheFile::default()
    };
    cache::write(&app.calendar_dir(), &c).unwrap();
}

pub fn record(app: &App, at: i64) -> (String, PathBuf) {
    let b: Value = serde_json::from_str(&app.begin_json(
        at,
        &mic_json().to_string(),
        &audio_json().to_string(),
    ))
    .unwrap();
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 60_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    (id, dir)
}

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
