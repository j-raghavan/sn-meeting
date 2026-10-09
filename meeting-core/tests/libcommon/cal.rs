//! Calendar fixtures (F18): a links file with one link, its cache as a refresh leaves it, and
//! recordings made at given times.
use super::*;
use meeting::app::App;
use meeting::calendar::cache::{self, CacheFile, LinkStatus};
use meeting::calendar::ics::{Attendee, Event};
use serde_json::Value;
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
