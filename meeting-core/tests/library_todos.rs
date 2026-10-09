//! F6-FR1 / F6-FR2: open to-dos come from every sidecar, and ticking one sticks.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use serde_json::{Value, json};

const MIN: i64 = 60_000;

/// A finished meeting at `T0 + i` minutes with to-dos `(id, text, done)`.
fn meeting_with(app: &App, e: &Env, i: i64, todos: &[(&str, &str, bool)]) -> String {
    let id = begin(app, T0 + i * MIN)["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1000, "");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    sc.todos = todos
        .iter()
        .map(|(tid, text, done)| {
            serde_json::from_value(json!({"id": tid, "text": text, "owner": "me", "due": null,
                "done": done, "firstSeen": sc.started_at}))
            .unwrap()
        })
        .collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    id
}

fn todo_ids(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t["todoId"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn ticking_one_of_three_leaves_two_and_survives_a_restart() {
    // F6-AC2.
    let e = Env::new();
    let app = e.app();
    let a = meeting_with(
        &app,
        &e,
        0,
        &[("a1", "Send the deck", false), ("a2", "Book room", false)],
    );
    let b = meeting_with(&app, &e, 1, &[("b1", "Call Priya", false)]);
    assert_eq!(todo_ids(&parse(&app.open_todos_json())), ["b1", "a1", "a2"]);
    assert_eq!(
        parse(&app.set_todo_done_json(&a, "a1", true)),
        json!({"ok": true})
    );
    assert_eq!(todo_ids(&parse(&app.open_todos_json())), ["b1", "a2"]);
    let restarted = e.app();
    assert_eq!(todo_ids(&parse(&restarted.open_todos_json())), ["b1", "a2"]);
    assert_eq!(parse(&restarted.list_recent_json(5))[1]["openTodoCount"], 1);
    restarted.set_todo_done_json(&a, "a1", false);
    assert_eq!(
        todo_ids(&parse(&restarted.open_todos_json())),
        ["b1", "a1", "a2"]
    );
    let first = &parse(&app.open_todos_json())[0];
    assert_eq!(
        *first,
        json!({"meetingId": b, "meetingStartedAt": "2026-10-07T21:18:06-07:00", "meetingTitle": null,
               "todoId": "b1", "text": "Call Priya", "owner": "me", "due": null})
    );
}

#[test]
fn a_todo_from_the_sixth_meeting_is_still_shown() {
    // F6-AC3.
    let e = Env::new();
    let app = e.app();
    let oldest = meeting_with(
        &app,
        &e,
        0,
        &[("old", "Renew contract", false), ("x", "done one", true)],
    );
    for i in 1..6 {
        meeting_with(&app, &e, i, &[]);
    }
    let recent = parse(&app.list_recent_json(5));
    assert!(
        recent
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["id"] != oldest.as_str())
    );
    let todos = parse(&app.open_todos_json());
    assert_eq!(todo_ids(&todos), ["old"]);
    assert_eq!(todos[0]["meetingStartedAt"], "2026-10-07T21:17:06-07:00");
}

#[test]
fn todos_from_previous_folders_count_and_unreadable_sidecars_are_skipped() {
    let e = Env::new();
    let app = e.app();
    meeting_with(&app, &e, 0, &[("p1", "from EXPORT", false)]);
    app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#);
    let dir = e.folder("Document/Meetings");
    std::fs::write(dir.join("meeting-20261009-100000.json"), "{broken").unwrap();
    assert_eq!(todo_ids(&parse(&app.open_todos_json())), ["p1"]);
}

#[test]
fn bad_targets_are_errors() {
    let e = Env::new();
    let app = e.app();
    let a = meeting_with(&app, &e, 0, &[("a1", "t", false)]);
    assert_eq!(
        parse(&app.set_todo_done_json(&a, "nope", true)),
        json!({"error": "to-do not found"})
    );
    assert_eq!(
        parse(&app.set_todo_done_json("meeting-20200101-000000", "a1", true)),
        json!({"error": "meeting not found"})
    );
    assert_eq!(
        parse(&app.set_todo_done_json("..", "a1", true)),
        json!({"error": "invalid meeting id"})
    );
    assert_eq!(
        parse(&e.app().open_todos_json()).as_array().unwrap().len(),
        1
    );
}
