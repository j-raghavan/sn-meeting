//! F18-FR7 / FR14 / F18-AC7: `nativeAgenda`'s days. Events from the cache and meetings from
//! the library in time order; a linked meeting folded into its event's row with its marks; a
//! recording outside any event as an unscheduled row; the current event with "From last time";
//! the week's dots; and with no link set, recordings alone.
mod libcommon;

use libcommon::cal::*;
use libcommon::*;
use meeting::sidecar::Todo;
use serde_json::{Value, json};

const DAY: i64 = 24 * 60 * MIN;

fn todo(id: &str, done: bool) -> Todo {
    Todo {
        id: id.into(),
        text: format!("to-do {id}"),
        owner: None,
        due: None,
        done,
        first_seen: String::new(),
        extra: Default::default(),
    }
}

fn day<'a>(a: &'a Value, d: &str) -> &'a Value {
    a["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["day"] == d)
        .unwrap()
}

#[test]
fn a_day_of_events_and_recordings() {
    let env = Env::new();
    let app = env.app();
    let mut sync = ev("sync", "Weekly sync", TEN, TEN + 30 * MIN);
    sync.recurring = true;
    let mut last = ev(
        "sync",
        "Weekly sync",
        TEN - 7 * DAY,
        TEN - 7 * DAY + 30 * MIN,
    );
    last.recurring = true;
    last.event_key = format!("{}:sync:last", sync.event_key.split(':').next().unwrap());
    let lunch = ev("lunch", "Lunch", TEN + 120 * MIN, TEN + 180 * MIN);
    let mut offsite = ev("off", "Offsite", TEN - 600 * MIN, TEN + 840 * MIN);
    offsite.all_day = true;
    let tomorrow = ev("tmr", "Planning", TEN + DAY, TEN + DAY + 60 * MIN);
    calendar(
        &env,
        &app,
        vec![last.clone(), offsite, sync.clone(), lunch, tomorrow],
    );
    // Last week's sync has two open to-dos; today's sync is being recorded; a recording at 15:00
    // is outside any event.
    let (old, dir) = record(&app, last.start_ms + MIN);
    let mut sc = read_sidecar(&dir, &old);
    sc.todos = vec![todo("a", false), todo("b", false), todo("c", true)];
    meeting::sidecar::write(&dir, &sc).unwrap();
    let (now_id, dir) = record(&app, TEN + MIN);
    let mut sc = read_sidecar(&dir, &now_id);
    sc.summary = Some("Minutes".into());
    sc.todos = vec![todo("x", true)];
    meeting::sidecar::write(&dir, &sc).unwrap();
    let (loose, _) = record(&app, TEN + 300 * MIN);

    let a = app
        .agenda_at("2026-10-05", "2026-10-11", TEN + 10 * MIN)
        .unwrap();
    assert_eq!(a["hasLinks"], true);
    assert_eq!(a["today"], "2026-10-09");
    let dots: Vec<bool> = a["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["dot"].as_bool().unwrap())
        .collect();
    assert_eq!(dots, [false, false, false, false, true, true, false]);
    let fri = day(&a, "2026-10-09");
    assert_eq!(fri["allDay"][0]["title"], "Offsite");
    let rows = fri["rows"].as_array().unwrap();
    let titles: Vec<&str> = rows
        .iter()
        .map(|r| r["title"].as_str().unwrap_or("-"))
        .collect();
    assert_eq!(titles, ["Weekly sync", "Lunch", "-"]);
    let s = &rows[0];
    assert_eq!(s["kind"], "event");
    assert_eq!(s["current"], true);
    assert_eq!(
        s["meetingId"],
        now_id.as_str(),
        "the meeting folds into its event"
    );
    assert_eq!(
        s["marks"],
        json!({"recording": true, "notes": false, "minutes": true, "todosOpen": 0, "todosTotal": 1,
               "status": "recorded", "audioLost": false})
    );
    assert_eq!(s["people"], "Priya Shah");
    assert_eq!(s["calendar"], "Work");
    let from: Vec<&str> = s["fromLastTime"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["todoId"].as_str().unwrap())
        .collect();
    assert_eq!(from, ["a", "b"], "F18-FR11 in the current box");
    assert_eq!(rows[1]["current"], false);
    assert!(rows[1].get("fromLastTime").is_none());
    assert_eq!(
        rows[1]["marks"]["recording"], false,
        "nothing recorded for lunch"
    );
    let u = &rows[2];
    assert_eq!(
        (u["kind"].as_str(), u["unscheduled"].as_bool()),
        (Some("meeting"), Some(true))
    );
    assert_eq!(u["meetingId"], loose.as_str());
    assert_eq!(u["endMs"], TEN + 300 * MIN + 60_000);
    assert_eq!(u["durationMs"], 60_000);
    assert_eq!(u["spend"]["label"], "—", "nothing spent yet");
    assert_eq!(s["durationMs"], 60_000, "the linked meeting's length");
    assert_eq!(rows[1]["durationMs"], Value::Null);
    // Tomorrow: not current, whatever the time.
    assert_eq!(day(&a, "2026-10-10")["rows"][0]["current"], false);
}

#[test]
fn no_link_builds_the_day_from_recordings_alone() {
    let env = Env::new();
    let app = env.app();
    let (id, _) = record(&app, TEN);
    let a = app
        .agenda_at("2026-10-09", "2026-10-09", TEN + MIN)
        .unwrap();
    assert_eq!(a["hasLinks"], false, "F18-AC7");
    assert_eq!(a["updatedAtMs"], Value::Null);
    let fri = day(&a, "2026-10-09");
    assert_eq!(fri["dot"], true);
    assert_eq!(fri["rows"][0]["meetingId"], id.as_str());
    assert_eq!(fri["rows"][0]["unscheduled"], true);
    assert!(fri["allDay"].as_array().unwrap().is_empty());
}

#[test]
fn a_linked_meeting_whose_event_is_gone_keeps_its_title() {
    let env = Env::new();
    let app = env.app();
    calendar(
        &env,
        &app,
        vec![ev("gone", "Old event", TEN, TEN + 30 * MIN)],
    );
    let (id, dir) = record(&app, TEN + MIN);
    calendar(&env, &app, vec![]);
    let a = app.agenda_at("2026-10-09", "2026-10-09", TEN).unwrap();
    let r = &day(&a, "2026-10-09")["rows"][0];
    assert_eq!(r["meetingId"], id.as_str());
    assert_eq!(r["unscheduled"], false);
    assert_eq!(r["title"], "Old event");
    let mut sc = read_sidecar(&dir, &id);
    sc.title = None;
    meeting::sidecar::write(&dir, &sc).unwrap();
    let a = app.agenda_at("2026-10-09", "2026-10-09", TEN).unwrap();
    assert_eq!(
        day(&a, "2026-10-09")["rows"][0]["title"],
        "Old event",
        "from the sidecar's event"
    );
}

#[test]
fn bad_days_are_refused() {
    let env = Env::new();
    let app = env.app();
    for (a, b) in [
        ("x", "2026-10-09"),
        ("2026-10-09", "2026-10-08"),
        ("2026-01-01", "2026-12-31"),
    ] {
        let r: Value = serde_json::from_str(&app.agenda_json(a, b)).unwrap();
        assert!(r["error"].is_string(), "{a}..{b}: {r}");
    }
    let r: Value = serde_json::from_str(&app.agenda_json("2026-10-09", "2026-10-09")).unwrap();
    assert_eq!(r["days"].as_array().unwrap().len(), 1);
}

#[test]
fn people_lines_are_short() {
    let mut e = ev("p", "P", TEN, TEN + MIN);
    e.attendees = (0..6)
        .map(|i| meeting::calendar::ics::Attendee {
            name: (i != 1).then(|| format!("N{i}")),
            email: Some(format!("e{i}@x.test")),
        })
        .collect();
    assert_eq!(meeting::app::agenda_people(&e), "N0, e1@x.test, N2, N3 +2");
}
