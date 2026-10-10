//! F19-FR4: what goes in the index from a sidecar, recognised notes and an event, and how the
//! user's words match it (case, accents, prefixes, phrases, kinds).
use meeting::notes_text::{Line, Region};
use meeting::search::query::{fts_query, matching};
use meeting::search::units::{Attendee, event_units, meeting_units};
use meeting::search::{EventDoc, Group, GroupKind, Index, Kind};
use meeting::sidecar::{self, Section, Segment, Sidecar, Todo};
use serde_json::json;

fn seg(start: u64, dur: Option<u64>, text: Option<&str>) -> Segment {
    Segment {
        file: String::new(),
        start_offset_ms: start,
        duration_ms: dur,
        text: text.map(str::to_owned),
        flag: None,
        extra: Default::default(),
    }
}

fn todo(id: &str, text: &str) -> Todo {
    Todo {
        id: id.into(),
        text: text.into(),
        owner: Some("Priya".into()),
        due: None,
        done: false,
        first_seen: String::new(),
        extra: Default::default(),
    }
}

fn sample() -> Sidecar {
    let mut sc = sidecar::blank("meeting-20261007-211706");
    sc.segments = vec![
        // 40 chars over 40 s: the second sentence starts at char 20, so at 20 s.
        seg(
            0,
            Some(40_000),
            Some("We met the team ok. The Café budget up!!"),
        ),
        seg(900_000, None, Some("(no speech detected)")),
        seg(1_000_000, None, Some("Résumé review later")),
        seg(2_000_000, Some(1), None),
        seg(3_000_000, Some(1), Some("  ")),
        seg(4_000_000, Some(1), Some("(audio missing)")),
    ];
    sc.summary = Some("Budget talk".into());
    sc.decisions = vec!["Freeze the budget".into(), "  ".into()];
    sc.sections = vec![Section {
        heading: "Risks".into(),
        body: "Hiring plan slips".into(),
        extra: Default::default(),
    }];
    sc.todos = vec![todo("t1", "Send the BUDGET deck")];
    sc.extra.insert(
        "marks".into(),
        json!([{"id": "m-1", "atMs": 761000, "label": "budget question"},
               {"id": "m-2", "atMs": 5, "label": null}, "junk"]),
    );
    sc
}

fn notes() -> Vec<Region> {
    vec![Region {
        canvas: 1,
        region: 1,
        rect: [0.0, 0.0, 10.0, 10.0],
        lines: vec![
            Line {
                text: "Q3 budget ↑".into(),
                bbox: [1.0, 2.0, 6.0, 2.5],
                stroke_ids: vec![4, 5],
                at_ms: Some(761_000),
                extra: Default::default(),
            },
            Line {
                text: "".into(),
                bbox: [0.0; 4],
                stroke_ids: vec![],
                at_ms: None,
                extra: Default::default(),
            },
        ],
        extra: Default::default(),
    }]
}

fn group(id: &str) -> Group {
    Group {
        kind: GroupKind::Meeting,
        id: id.into(),
        title: None,
        start: String::new(),
        start_ms: 0,
    }
}

#[test]
fn a_meeting_becomes_its_units() {
    let u = meeting_units(&sample(), &notes(), &[1]);
    let ids: Vec<&str> = u.iter().map(|u| u.unit_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "t:0:0",
            "t:0:1",
            "t:2:0",
            "mark:m-1",
            "n:1:0",
            "summary",
            "decision:0",
            "section:0",
            "todo:t1"
        ]
    );
    assert_eq!(u[0].at_ms, Some(0));
    assert_eq!(
        u[1].at_ms,
        Some(20_000),
        "interpolated by character position"
    );
    assert_eq!(
        u[2].at_ms,
        Some(1_000_000),
        "no duration: the segment start"
    );
    assert_eq!(u[3].kind, Kind::Transcript);
    assert_eq!(u[3].at_ms, Some(761_000));
    let n = &u[4];
    assert_eq!(
        (n.kind, n.region, n.bbox, n.at_ms, n.changed),
        (
            Kind::Notes,
            Some(1),
            Some([1.0, 2.0, 6.0, 2.5]),
            Some(761_000),
            true
        )
    );
    assert_eq!(n.stroke_ids, vec![4, 5]);
    assert_eq!(u[7].text, "Risks\nHiring plan slips");
    assert_eq!(u[8].kind, Kind::Todo);
    assert!(!meeting_units(&sample(), &notes(), &[])[4].changed);
}

#[test]
fn an_event_is_one_unit() {
    let e = EventDoc {
        event_key: "l:uid:-".into(),
        title: "Budget sync".into(),
        start: "2026-10-09T10:00:00-07:00".into(),
        location: Some("Room 4".into()),
        attendees: vec![
            Attendee {
                name: Some("Priya".into()),
                email: None,
            },
            Attendee {
                name: None,
                email: Some("sam@example.com".into()),
            },
            Attendee {
                name: None,
                email: None,
            },
        ],
        agenda: Some("  ".into()),
    };
    let u = event_units(&e);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].text, "Budget sync\nRoom 4\nPriya\nsam@example.com");
    let blank = EventDoc {
        title: String::new(),
        location: None,
        attendees: vec![],
        agenda: None,
        ..e
    };
    assert!(event_units(&blank).is_empty());
}

fn indexed() -> Index {
    let mut ix = Index::in_memory();
    ix.put(&group("a"), "s", &meeting_units(&sample(), &notes(), &[]))
        .unwrap();
    let e = EventDoc {
        event_key: "l:uid:-".into(),
        title: "Budget sync".into(),
        start: "2026-10-09T10:00:00-07:00".into(),
        location: None,
        attendees: vec![],
        agenda: None,
    };
    let g = Group {
        kind: GroupKind::Event,
        id: e.event_key.clone(),
        title: Some(e.title.clone()),
        start: e.start.clone(),
        start_ms: 9,
    };
    ix.put(&g, "e", &event_units(&e)).unwrap();
    ix
}

fn kinds_for(ix: &Index, q: &str, kind: Option<Kind>) -> Vec<Kind> {
    let fts = fts_query(q).unwrap();
    let mut k: Vec<Kind> = matching(ix, &fts, kind)
        .unwrap()
        .into_iter()
        .map(|m| m.kind)
        .collect();
    k.sort_by_key(|k| k.id());
    k
}

#[test]
fn matching_is_case_and_accent_insensitive_with_a_prefix_on_the_last_term() {
    let ix = indexed();
    // transcript line, mark, note line, summary, decision, to-do, event.
    assert_eq!(kinds_for(&ix, "budget", None).len(), 7);
    assert_eq!(kinds_for(&ix, "BUDG", None).len(), 7, "prefix and case");
    assert_eq!(kinds_for(&ix, "bu", None).len(), 7, "two-letter prefix");
    assert_eq!(kinds_for(&ix, "cafe", None), [Kind::Transcript]);
    assert_eq!(kinds_for(&ix, "resume", None), [Kind::Transcript]);
    assert_eq!(kinds_for(&ix, "RÉSUMÉ", None), [Kind::Transcript]);
    assert!(
        kinds_for(&ix, "udget", None).is_empty(),
        "no substrings (OQ27)"
    );
    assert!(
        kinds_for(&ix, "budg cafe", None).is_empty(),
        "only the last term is a prefix"
    );
    assert_eq!(kinds_for(&ix, "cafe budg", None), [Kind::Transcript]);
    assert_eq!(kinds_for(&ix, "q3 budget", None), [Kind::Notes]);
}

#[test]
fn phrases_and_kinds() {
    let ix = indexed();
    assert_eq!(
        kinds_for(&ix, "\"freeze the budget\"", None),
        [Kind::Minutes]
    );
    assert!(kinds_for(&ix, "\"the freeze budget\"", None).is_empty());
    assert_eq!(kinds_for(&ix, "budget", Some(Kind::Notes)), [Kind::Notes]);
    assert_eq!(kinds_for(&ix, "budget", Some(Kind::Todo)), [Kind::Todo]);
    assert_eq!(kinds_for(&ix, "budget", Some(Kind::Event)), [Kind::Event]);
    assert_eq!(
        kinds_for(&ix, "budget", Some(Kind::Minutes)),
        [Kind::Minutes, Kind::Minutes]
    );
    assert_eq!(
        kinds_for(&ix, "hiring", None),
        [Kind::Minutes],
        "section body"
    );
    assert_eq!(
        kinds_for(&ix, "risks", None),
        [Kind::Minutes],
        "section heading"
    );
    // FTS5 syntax typed by the user is plain text.
    assert!(kinds_for(&ix, "budget OR NEAR(x)", None).is_empty());
    let m = matching(&ix, &fts_query("budget").unwrap(), None).unwrap();
    assert!(m.windows(2).all(|w| w[0].rank <= w[1].rank), "best first");
    assert!(m.iter().any(|h| h.group == "e:l:uid:-"));
}
