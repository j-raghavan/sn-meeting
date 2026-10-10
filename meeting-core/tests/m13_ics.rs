//! F18-FR5 / F18-AC1: Google- and Outlook-style feeds expanded for the week of 2026-10-05, in
//! device-local time (PDT here): weekly RRULE with EXDATE and a RECURRENCE-ID override, a
//! Windows TZID, RDATE, all-day, cancelled, transparent, floating and unknown-zone events, and
//! the caps that bound a hostile feed (EC25).
use chrono::{FixedOffset, TimeZone};
use meeting::calendar::ics::{self, Event};
use meeting::tz::FixedTz;

const PDT: i32 = -7 * 3600;

fn tz() -> FixedTz {
    FixedTz(FixedOffset::east_opt(PDT).unwrap())
}

fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
    FixedOffset::east_opt(PDT)
        .unwrap()
        .with_ymd_and_hms(y, mo, d, h, mi, 0)
        .unwrap()
        .timestamp_millis()
}

fn week() -> (i64, i64) {
    (ms(2026, 10, 5, 0, 0), ms(2026, 10, 12, 0, 0))
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/calendar/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn rows(evs: &[Event]) -> Vec<(String, String, String)> {
    evs.iter()
        .map(|e| (e.start.clone(), e.end.clone(), e.title.clone()))
        .collect()
}

fn s(a: &str, b: &str, t: &str) -> (String, String, String) {
    (a.into(), b.into(), t.into())
}

#[test]
fn google_week_matches_the_expected_list() {
    let x = ics::expand(&fixture("google.ics"), "0123abcd", &tz(), week()).unwrap();
    assert_eq!(x.name.as_deref(), Some("Work"));
    assert_eq!(
        rows(&x.events),
        [
            s(
                "2026-10-05T10:00:00-07:00",
                "2026-10-05T10:30:00-07:00",
                "Team stand-up"
            ),
            s(
                "2026-10-06T00:00:00-07:00",
                "2026-10-07T00:00:00-07:00",
                "Team offsite"
            ),
            // Wed 7 is an EXDATE; Fri 9 is moved from 10:00 to 14:00.
            s(
                "2026-10-09T14:00:00-07:00",
                "2026-10-09T14:30:00-07:00",
                "Team stand-up (moved)"
            ),
            s(
                "2026-10-10T09:00:00-07:00",
                "2026-10-10T09:30:00-07:00",
                "Floating coffee"
            ),
            s(
                "2026-10-10T12:00:00-07:00",
                "2026-10-10T13:00:00-07:00",
                "Unknown zone"
            ),
        ]
    );
    // No start, no UID: skipped and counted; the cancelled instance is simply dropped.
    assert_eq!(x.skipped, 2);
    let mon = &x.events[0];
    assert_eq!(
        mon.event_key,
        "0123abcd:standup-series@google.com:20261005T170000Z"
    );
    assert_eq!(mon.series_uid, "standup-series@google.com");
    assert!(mon.recurring && !mon.all_day && !mon.transparent() && !mon.tz_unknown);
    assert_eq!(
        mon.recurrence_id.as_deref(),
        Some("2026-10-05T10:00:00-07:00")
    );
    assert_eq!(mon.location.as_deref(), Some("Room 4"));
    let names: Vec<_> = mon
        .attendees
        .iter()
        .map(|a| (a.name.as_deref(), a.email.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            (Some("Priya Shah"), Some("priya@example.com")),
            (Some("Ana Ruiz"), Some("ana@example.com")),
            (None, Some("noname@example.com")),
        ]
    );
    assert_eq!(
        mon.organizer.as_ref().unwrap().name.as_deref(),
        Some("Priya Shah")
    );
    assert!(
        mon.agenda
            .as_deref()
            .unwrap()
            .starts_with("Agenda:\n1. Blockers")
    );
    assert_eq!(mon.status.as_deref(), Some("confirmed"));
    let moved = &x.events[2];
    assert_eq!(
        moved.event_key,
        "0123abcd:standup-series@google.com:20261009T170000Z"
    );
    assert_eq!(
        moved.recurrence_id.as_deref(),
        Some("2026-10-09T10:00:00-07:00")
    );
    assert_eq!(moved.location.as_deref(), Some("Room 9"));
    let offsite = &x.events[1];
    assert!(offsite.all_day && offsite.transparent() && !offsite.recurring);
    assert_eq!(offsite.event_key, "0123abcd:offsite@google.com:-");
    assert!(x.events[4].tz_unknown, "EC26");
    assert!(
        !x.events[3].tz_unknown,
        "floating is device local, not unknown"
    );
}

#[test]
fn outlook_windows_zone_and_rdate() {
    let x = ics::expand(&fixture("outlook.ics"), "fedc0987", &tz(), week()).unwrap();
    assert_eq!(
        rows(&x.events),
        [
            s(
                "2026-10-05T15:00:00-07:00",
                "2026-10-05T16:00:00-07:00",
                "Interview loop"
            ),
            s(
                "2026-10-07T15:00:00-07:00",
                "2026-10-07T16:00:00-07:00",
                "Interview loop"
            ),
            s(
                "2026-10-08T09:00:00-07:00",
                "2026-10-08T09:30:00-07:00",
                "1:1 with Sam"
            ),
        ]
    );
    assert_eq!(x.skipped, 0);
    assert!(
        x.events.iter().all(|e| !e.tz_unknown),
        "Windows names resolve"
    );
    assert!(x.events[1].event_key.ends_with(":20261007T220000Z"));
}

#[test]
fn a_winter_instance_uses_standard_time() {
    // The Outlook series runs to Dec 31: Dec 3 is PST (-08:00), 17:00Z, read in a fixed PDT test
    // zone as 10:00-07:00.
    let win = (ms(2026, 12, 3, 0, 0), ms(2026, 12, 4, 0, 0));
    let x = ics::expand(&fixture("outlook.ics"), "fedc0987", &tz(), win).unwrap();
    assert_eq!(x.events.len(), 1);
    assert_eq!(x.events[0].start, "2026-12-03T10:00:00-07:00");
}

#[test]
fn the_window_is_minus_90_to_plus_60_days() {
    let now = ms(2026, 10, 9, 15, 30);
    let (a, b) = ics::window(now, &tz());
    assert_eq!(a, ms(2026, 7, 11, 0, 0));
    assert_eq!(b, ms(2026, 12, 9, 0, 0));
    let x = ics::expand(&fixture("google.ics"), "0123abcd", &tz(), (a, b)).unwrap();
    // The stand-up runs from Sep 7 (Mon/Wed/Fri), endless: up to the window's end.
    let n = x
        .events
        .iter()
        .filter(|e| e.series_uid == "standup-series@google.com")
        .count();
    assert!(n > 30 && n < 60, "{n}");
}

fn daily_since_1970(n_series: usize) -> String {
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
    for i in 0..n_series {
        s.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:d{i}\r\nDTSTART:19700101T090000Z\r\nDTEND:19700101T091500Z\r\nRRULE:FREQ=DAILY\r\nSUMMARY:Daily {i}\r\nEND:VEVENT\r\n"
        ));
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

#[test]
fn hostile_feeds_are_bounded() {
    let win = ics::window(ms(2026, 10, 9, 12, 0), &tz());
    // A daily meeting since 1970 reaches today within its own budget.
    let x = ics::expand(&daily_since_1970(1), "aaaaaaaa", &tz(), win).unwrap();
    assert!(x.events.len() > 140, "{}", x.events.len());
    // Many such series exhaust the feed's budget: later ones are skipped and counted, and the
    // work stays bounded.
    let started = std::time::Instant::now();
    let x = ics::expand(&daily_since_1970(15), "aaaaaaaa", &tz(), win).unwrap();
    assert!(x.skipped > 0, "the feed budget stopped some series");
    assert!(started.elapsed().as_secs() < 20);
    // A secondly rule inside the window keeps at most 2 000 instances.
    let now = chrono::Utc::now().format("%Y%m%dT000000Z");
    let sec = format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:s\r\nDTSTART:{now}\r\nDURATION:PT1S\r\nRRULE:FREQ=SECONDLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    );
    let win = ics::window(chrono::Utc::now().timestamp_millis(), &tz());
    let x = ics::expand(&sec, "aaaaaaaa", &tz(), win).unwrap();
    assert_eq!(x.events.len(), ics::MAX_PER_SERIES);
    assert!(ics::expand("BEGIN:VCARD\r\nEND:VCARD\r\n", "a", &tz(), win).is_none());
}

#[test]
fn long_descriptions_are_capped() {
    let long = "x".repeat(20_000);
    let f = format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:l\r\nDTSTART:20261007T170000Z\r\nDTEND:20261007T180000Z\r\nDESCRIPTION:{long}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    );
    let x = ics::expand(&f, "aaaaaaaa", &tz(), week()).unwrap();
    assert_eq!(x.events[0].agenda.as_ref().unwrap().len(), ics::MAX_AGENDA);
    assert_eq!(x.events[0].title, "");
}
