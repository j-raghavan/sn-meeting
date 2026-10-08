//! F5-FR5: the model's JSON is validated and capped, then merged into the sidecar: TODOs by
//! normalised text, done never dropped, unreturned kept, a user's title kept.
use meeting::sidecar::{self, Sidecar, TitleSource};
use meeting::summarise::{self, Extraction, NewTodo, merge, parse};
use serde_json::json;

fn sc() -> Sidecar {
    Sidecar {
        started_at: "2026-10-07T21:17:06-07:00".into(),
        ..sidecar::blank("meeting-20261007-211706")
    }
}

fn todo(text: &str, owner: Option<&str>, due: Option<&str>) -> NewTodo {
    NewTodo {
        text: text.into(),
        owner: owner.map(Into::into),
        due: due.map(Into::into),
    }
}

fn x(title: Option<&str>, todos: Vec<NewTodo>) -> Extraction {
    Extraction {
        title: title.map(Into::into),
        summary: "S".into(),
        decisions: vec!["D".into()],
        todos,
        sections: vec![("Blockers".into(), "- none".into())],
    }
}

fn counter() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("id{n}")
    }
}

#[test]
fn a_done_todo_stays_done_when_the_model_rewords_it() {
    // F5-AC5.
    let mut s = sc();
    let mut ids = counter();
    merge(
        &mut s,
        x(None, vec![todo("Send the deck to Priya", Some("me"), None)]),
        "t1",
        &mut ids,
    );
    s.todos[0].done = true;
    let again = x(
        None,
        vec![todo("send the deck to Priya!", None, Some("2026-10-09"))],
    );
    merge(&mut s, again, "t2", &mut ids);
    assert_eq!(s.todos.len(), 1);
    let t = &s.todos[0];
    assert!(t.done);
    assert_eq!((t.id.as_str(), t.first_seen.as_str()), ("id1", "t1"));
    assert_eq!(t.text, "Send the deck to Priya", "stored wording kept");
    assert_eq!(
        t.owner.as_deref(),
        Some("me"),
        "owner kept when the new one is null"
    );
    assert_eq!(
        t.due.as_deref(),
        Some("2026-10-09"),
        "a new due date is taken"
    );
}

#[test]
fn an_unreturned_todo_is_kept_and_new_ones_get_fresh_ids() {
    // F5-AC6, fresh-id stability.
    let mut s = sc();
    let mut ids = counter();
    let first = vec![
        todo("Book the room", None, None),
        todo("Call Arjun", None, None),
    ];
    merge(&mut s, x(None, first), "t1", &mut ids);
    merge(
        &mut s,
        x(None, vec![todo("Write the brief", Some("Priya"), None)]),
        "t2",
        &mut ids,
    );
    let got: Vec<_> = s
        .todos
        .iter()
        .map(|t| (t.id.as_str(), t.text.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("id1", "Book the room"),
            ("id2", "Call Arjun"),
            ("id3", "Write the brief")
        ]
    );
    assert_eq!(s.todos[2].first_seen, "t2");
    // A result naming the same action twice adds it once.
    let twice = vec![todo("Ship it", None, None), todo("ship it.", None, None)];
    merge(&mut s, x(None, twice), "t3", &mut ids);
    assert_eq!(s.todos.len(), 4);
    // Summary, decisions and sections are replaced.
    assert_eq!(s.summary.as_deref(), Some("S"));
    assert_eq!(s.decisions, ["D"]);
    assert_eq!(s.sections[0].heading, "Blockers");
}

#[test]
fn a_user_title_is_kept_and_an_untitled_meeting_gains_one() {
    // F5-AC11.
    let mut s = sc();
    merge(&mut s, x(Some("Deck review"), vec![]), "t", counter());
    assert_eq!(
        (s.title.as_deref(), s.title_source),
        (Some("Deck review"), Some(TitleSource::Auto))
    );
    merge(&mut s, x(Some("Renamed by model"), vec![]), "t", counter());
    assert_eq!(
        s.title.as_deref(),
        Some("Renamed by model"),
        "auto titles refresh"
    );
    s.title = Some("Mine".into());
    s.title_source = Some(TitleSource::User);
    merge(&mut s, x(Some("Other"), vec![]), "t", counter());
    assert_eq!(
        (s.title.as_deref(), s.title_source),
        (Some("Mine"), Some(TitleSource::User))
    );
    // No title returned: an untitled meeting stays untitled.
    let mut s = sc();
    merge(&mut s, x(None, vec![]), "t", counter());
    assert_eq!((s.title, s.title_source), (None, None));
}

#[test]
fn replies_are_validated_and_capped() {
    let reply = json!({
        "title": "Weekly sync.\nsecond line", "summary": "  s  ", "extra": 1,
        "decisions": ["", "d", "x".repeat(600)],
        "todos": [{"text": " a ", "owner": "", "due": "Friday"},
                  {"text": "", "owner": "x", "due": null},
                  {"text": "b", "owner": "o".repeat(90), "due": "2026-10-09", "more": true}],
        "sections": [{"heading": "", "body": "dropped"},
                     {"heading": "H", "body": "b".repeat(9000)}]
    });
    let fenced = format!("```json\n{reply}\n```");
    let e = parse(&fenced).unwrap();
    assert_eq!(e.title.as_deref(), Some("Weekly sync. second line"));
    assert_eq!(e.summary, "s");
    assert_eq!(e.decisions.len(), 2);
    assert_eq!(e.decisions[1].chars().count(), summarise::MAX_ITEM);
    assert_eq!(
        e.todos[0],
        todo("a", None, None),
        "empty owner and a weekday due are null"
    );
    assert_eq!(
        e.todos[1].owner.as_ref().unwrap().chars().count(),
        summarise::MAX_OWNER
    );
    assert_eq!(e.todos[1].due.as_deref(), Some("2026-10-09"));
    assert_eq!(e.sections.len(), 1);
    assert_eq!(e.sections[0].1.chars().count(), summarise::MAX_BODY);
    assert_eq!(parse(&format!("```{reply}```")).unwrap(), e);
    // Missing fields default; lists are capped at 100.
    let many: Vec<_> = (0..150)
        .map(|i| json!({"text": format!("t{i}"), "owner": null, "due": null}))
        .collect();
    let e = parse(&json!({"todos": many}).to_string()).unwrap();
    assert_eq!(
        (e.todos.len(), e.title, e.summary.as_str()),
        (summarise::MAX_LIST, None, "")
    );
    let long = "y".repeat(5000);
    assert_eq!(
        parse(&json!({"summary": long}).to_string())
            .unwrap()
            .summary
            .len(),
        4000
    );
    for bad in ["nope", "[]", r#"{"todos": [{"owner": "x"}]}"#] {
        let err = parse(bad).unwrap_err();
        assert!(err.starts_with("The summary wasn't valid"), "{bad}: {err}");
    }
}

#[test]
fn fenced_or_prose_wrapped_and_null_lists_parse() {
    let reply =
        r#"{"title": "Sync", "summary": "s", "decisions": null, "todos": null, "sections": null}"#;
    let want = parse(reply).unwrap();
    assert_eq!(
        (want.decisions.len(), want.todos.len(), want.sections.len()),
        (0, 0, 0)
    );
    for wrapped in [
        format!("```JSON\n{reply}\n```"),
        format!("```Json {reply} ```"),
        format!("Here is the summary:\n\n{reply}\n\nLet me know if you need more."),
        format!("Sure! ```json\n{reply}\n``` Done."),
    ] {
        assert_eq!(parse(&wrapped).unwrap(), want, "{wrapped}");
    }
    let e = parse(
        r#"{"summary": "s", "todos": [{"text": "a", "owner": null, "due": null}], "decisions": [null, "d"]}"#,
    );
    assert!(e.is_err(), "a null inside a list is still invalid: {e:?}");
}

#[test]
fn todos_with_no_words_are_dropped_when_parsed() {
    // A to-do that is only emoji or punctuation has no identity to merge by (F5-FR5), so it is
    // never stored; one with a word in it is kept as written.
    let reply = json!({"summary": "s", "todos": [
        {"text": "✅", "owner": null, "due": null},
        {"text": "...", "owner": null, "due": null},
        {"text": "🎉 — !", "owner": null, "due": null},
        {"text": "🎉 Ship it", "owner": null, "due": null}
    ]});
    let e = parse(&reply.to_string()).unwrap();
    assert_eq!(e.todos, vec![todo("🎉 Ship it", None, None)]);
}
