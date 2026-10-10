//! F19-FR2 / F19-AC1: results grouped by meeting or event, in F19-FR2's order, with source
//! labels, bold matches, times for timed hits and the filter chips; "Show more" by limit.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::search::query::Clock;
use meeting::sidecar::{Segment, Todo};
use serde_json::{Value, json};
use std::path::PathBuf;

const DAY: i64 = 86_400_000;

fn meeting(app: &App, at_ms: i64, word: &str) -> String {
    let b = begin(app, at_ms);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    app.finish_json(&id, 60_000, "");
    let mut sc = read_sidecar(&dir, &id);
    sc.title = Some(format!("Meeting {word}"));
    sc.segments = vec![Segment {
        file: sc.segments[0].file.clone(),
        start_offset_ms: 760_000,
        duration_ms: Some(10_000),
        text: Some(format!("We need the {word} today.")),
        flag: None,
        extra: Default::default(),
    }];
    sc.summary = Some(format!("The {word} was agreed"));
    sc.todos = vec![Todo {
        id: "t1".into(),
        text: format!("Send the {word} deck"),
        owner: None,
        due: None,
        done: false,
        first_seen: String::new(),
        extra: Default::default(),
    }];
    meeting::sidecar::write(&dir, &sc).unwrap();
    let p = meeting::notes_text::path(&dir, &id);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let notes = json!([{"region": 0, "rect": [0, 0, 100, 100], "lines": [
        {"text": format!("Q3 {word} ↑"), "box": [10, 20, 60, 25], "strokeIds": [4, 5],
         "atMs": 761000}]}]);
    std::fs::write(p, notes.to_string()).unwrap();
    id
}

fn events(app: &App, list: Value) {
    let d = app.calendar_dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("l1.json"), json!({"events": list}).to_string()).unwrap();
}

/// Three meetings a day apart from T0, and two events: one before them all is over, one upcoming.
fn setup(app: &App) -> (Vec<String>, Clock) {
    let ids = vec![
        meeting(app, T0, "budget"),
        meeting(app, T0 + DAY, "budget"),
        meeting(app, T0 + 2 * DAY, "budget"),
    ];
    events(
        app,
        json!([
            {"eventKey": "l1:past:-", "title": "Budget review", "start": "2026-10-08T10:00:00-07:00"},
            {"eventKey": "l1:soon:-", "title": "Budget planning", "start": "2026-10-12T09:00:00-07:00",
             "location": "Room 4"},
        ]),
    );
    app.index_update_json();
    // Today is the third meeting's day (2026-10-09, PDT), an hour after it began.
    let clock = Clock {
        now_ms: T0 + 2 * DAY + 3_600_000,
        today_start_ms: 1_791_529_200_000,
    };
    (ids, clock)
}

fn search(app: &App, q: &str, filter: &str, limit: i32, c: Clock) -> Value {
    parse(&app.search_at(q, filter, limit, c))
}

fn bold(hit: &Value) -> Vec<String> {
    hit["snippet"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["bold"] == true)
        .map(|s| s["text"].as_str().unwrap().to_owned())
        .collect()
}

/// F19-AC1.
#[test]
fn grouped_by_meeting_and_event_in_order_with_labels_and_bold() {
    let e = Env::new();
    let app = e.app();
    let (ids, c) = setup(&app);
    let r = search(&app, "budget", "all", 0, c);
    let order: Vec<&str> = r["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    // Today's meeting, the upcoming event, then earlier days newest first.
    assert_eq!(order, [&ids[2], "l1:soon:-", &ids[1], "l1:past:-", &ids[0]]);
    assert_eq!(r["totalGroups"], 5);
    assert_eq!(r["more"], false);
    assert_eq!(r["indexing"], false);
    let m = &r["groups"][0];
    assert_eq!(m["kind"], "meeting");
    assert_eq!(m["title"], "Meeting budget");
    assert_eq!(m["hitCount"], 4);
    let mut labels: Vec<&str> = m["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["label"].as_str().unwrap())
        .collect();
    labels.sort();
    assert_eq!(labels, ["MINUTES", "NOTES", "TO-DO", "TRANSCRIPT"]);
    for h in m["hits"].as_array().unwrap() {
        assert_eq!(bold(h), ["budget"], "{h}");
    }
    let ev = &r["groups"][1];
    assert_eq!(ev["kind"], "event");
    assert_eq!(ev["title"], "Budget planning");
    assert_eq!(ev["hits"][0]["label"], "EVENT");
    assert_eq!(bold(&ev["hits"][0]), ["Budget"]);
    assert_eq!(ev["hits"][0]["time"], Value::Null);

    // With Notes, only notes hits remain.
    let n = search(&app, "budget", "notes", 0, c);
    assert_eq!(n["groups"].as_array().unwrap().len(), 3);
    for g in n["groups"].as_array().unwrap() {
        assert_eq!(g["hits"].as_array().unwrap().len(), 1);
        assert_eq!(g["hits"][0]["kind"], "notes");
    }
    let events = search(&app, "budget", "events", 0, c);
    assert_eq!(events["groups"].as_array().unwrap().len(), 2);
    assert_eq!(
        search(&app, "budget", "todos", 0, c)["groups"][0]["hits"][0]["label"],
        "TO-DO"
    );
}

/// F19-AC2 (the hit's side): a notes line timed by its earliest stroke shows 0:12:41 and
/// carries its strokes and box for the outline.
#[test]
fn timed_hits_show_their_time_and_notes_carry_their_strokes() {
    let e = Env::new();
    let app = e.app();
    let (_, c) = setup(&app);
    let n = search(&app, "q3 budget", "notes", 0, c);
    let h = &n["groups"][0]["hits"][0];
    assert_eq!(h["time"], "0:12:41");
    assert_eq!(h["atMs"], 761_000);
    assert_eq!(h["strokeIds"], json!([4, 5]));
    assert_eq!(h["bbox"], json!([10.0, 20.0, 60.0, 25.0]));
    assert_eq!(h["region"], 0);
    assert_eq!(h["changed"], false);
    assert_eq!(bold(h), ["Q3", "budget"]);
    let t = search(&app, "budget", "transcript", 0, c);
    let h = &t["groups"][0]["hits"][0];
    assert_eq!(h["unitId"], "t:0:0");
    assert_eq!(h["time"], "0:12:40");
    let m = search(&app, "agreed", "minutes", 0, c);
    assert_eq!(m["groups"][0]["hits"][0]["time"], Value::Null);
    assert_eq!(m["groups"][0]["hits"][0]["unitId"], "summary");
}

#[test]
fn show_more_no_results_and_no_word() {
    let e = Env::new();
    let app = e.app();
    let (_, c) = setup(&app);
    let r = search(&app, "budget", "all", 2, c);
    assert_eq!(r["groups"].as_array().unwrap().len(), 2);
    assert_eq!(r["more"], true);
    assert_eq!(r["totalGroups"], 5);
    let r = search(&app, "budget", "all", 50, c);
    assert_eq!(r["more"], false);
    let none = search(&app, "zebra", "minutes", 0, c);
    assert_eq!(none["groups"], json!([]));
    assert_eq!(none["query"], "zebra");
    assert_eq!(none["filter"], "minutes");
    assert_eq!(search(&app, " !! ", "all", 0, c)["groups"], json!([]));
    assert_eq!(
        search(&app, "budget", "x", 0, c),
        json!({"error": "unknown filter"})
    );
    // Through the real clock too: the same groups, in some order.
    let live = parse(&app.search_json("budget", "all", 0));
    assert_eq!(live["totalGroups"], 5);
}

/// A group's hits are capped; the count says how many matched.
#[test]
fn many_hits_in_one_meeting_are_capped() {
    let e = Env::new();
    let app = e.app();
    let id = meeting(&app, T0, "budget");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    let text: Vec<String> = (0..30).map(|i| format!("Budget line {i}.")).collect();
    sc.segments[0].text = Some(text.join(" "));
    meeting::sidecar::write(&dir, &sc).unwrap();
    app.index_update_json();
    let r = parse(&app.search_json("budget", "transcript", 0));
    let g = &r["groups"][0];
    assert_eq!(g["hitCount"], 30);
    assert_eq!(
        g["hits"].as_array().unwrap().len(),
        meeting::search::query::MAX_HITS_PER_GROUP
    );
}

/// F19-FR7 (M12 hook): a stroke written on the canvas after recognition, inside a stored
/// region, marks that region's hits "changed since recognised"; the canvas's stamp alone
/// brings the meeting back into the index.
#[test]
fn a_stroke_written_since_recognition_marks_its_region_changed() {
    let e = Env::new();
    let app = e.app();
    let id = meeting(&app, T0, "budget");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    sc.extra.insert(
        "notesRecognisedAt".into(),
        json!("2026-10-07T21:30:00-07:00"),
    );
    sc.extra.insert("notes".into(), meeting::notes::field(&id));
    meeting::sidecar::write(&dir, &sc).unwrap();
    app.index_update_json();
    let first = parse(&app.search_json("q3", "notes", 0));
    assert_eq!(first["groups"][0]["hits"][0]["changed"], false);
    // A stroke inside region 0 ([0, 0, 100, 100]), written now: after notesRecognisedAt.
    let pts = [40.0, 40.0, 1.0, -1.0, 50.0, 45.0, 1.0, -1.0];
    assert!(
        parse(&app.ink_stroke_json(&id, "canvas", 0, 3.0, &pts))
            .get("error")
            .is_none()
    );
    app.ink_save_json(&id);
    app.index_update_json();
    let after = parse(&app.search_json("q3", "notes", 0));
    assert_eq!(after["groups"][0]["hits"][0]["changed"], true);
}
