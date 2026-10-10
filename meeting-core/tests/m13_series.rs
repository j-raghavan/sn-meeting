//! F18-FR11 / F18-AC5: the current instance of a recurring event shows "From last time": the
//! open to-dos of the most recent earlier meeting in its series, exactly those.
mod libcommon;

use libcommon::cal::{MIN, TEN, calendar, ev, record};
use libcommon::*;
use meeting::sidecar::Todo;

const WEEK: i64 = 7 * 24 * 60 * MIN;

fn todo(id: &str, text: &str, done: bool) -> Todo {
    Todo {
        id: id.into(),
        text: text.into(),
        owner: None,
        due: None,
        done,
        first_seen: String::new(),
        extra: Default::default(),
    }
}

/// The weekly sync's instance `n` weeks before (negative) or after TEN.
fn weekly(n: i64) -> meeting::calendar::ics::Event {
    let start = TEN + n * WEEK;
    let mut e = ev("sync", "Weekly sync", start, start + 30 * MIN);
    e.recurring = true;
    e.event_key = format!("{}:sync:{start}", e.event_key.split(':').next().unwrap());
    e
}

#[test]
fn from_last_time_lists_the_latest_meetings_open_to_dos() {
    let env = Env::new();
    let app = env.app();
    let instances: Vec<_> = (-3..=0).map(weekly).collect();
    calendar(&env, &app, instances.clone());
    // Three past meetings in the series, each linked by time; only the latest has open to-dos.
    let mut ids = Vec::new();
    for (i, w) in instances[..3].iter().enumerate() {
        let (id, dir) = record(&app, w.start_ms + 60_000);
        let mut sc = read_sidecar(&dir, &id);
        sc.todos = match i {
            0 => vec![todo("a", "Old open one", false)],
            1 => vec![todo("b", "Done already", true)],
            _ => vec![
                todo("c", "Send the deck", false),
                todo("d", "Done", true),
                todo("e", "Book room", false),
            ],
        };
        meeting::sidecar::write(&dir, &sc).unwrap();
        ids.push(id);
    }
    let current = &instances[3];
    let got: Vec<(String, String)> = app
        .from_last_time(current)
        .into_iter()
        .map(|t| (t.meeting_id, t.text))
        .collect();
    assert_eq!(
        got,
        [
            (ids[2].clone(), "Send the deck".to_owned()),
            (ids[2].clone(), "Book room".to_owned())
        ],
        "F18-AC5: exactly the latest meeting's open to-dos"
    );
    // Earlier in this series: newest first. The meeting recorded now is not earlier.
    let (now_id, _) = record(&app, current.start_ms + 2 * MIN);
    let series: Vec<String> = app
        .series_meetings(current)
        .into_iter()
        .map(|(m, _)| m.id)
        .collect();
    assert_eq!(series, [ids[2].clone(), ids[1].clone(), ids[0].clone()]);
    assert!(!series.contains(&now_id));
    // Ticking one there ticks it in its meeting (F6-FR2).
    let r = app.set_todo_done_json(&ids[2], "c", true);
    assert!(!r.contains("error"), "{r}");
    assert_eq!(app.from_last_time(current).len(), 1);
    // A different series has nothing from last time.
    let other = ev("other", "Other", TEN, TEN + MIN);
    assert!(app.from_last_time(&other).is_empty());
}
