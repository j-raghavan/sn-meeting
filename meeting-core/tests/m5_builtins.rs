//! F11-FR2: the six built-ins parse, share the standard layout, render every layout variable,
//! and survive a round trip through the file format.
use meeting::templates::{self, BUILTINS, parse, render, sample_context, to_file};

#[test]
fn six_builtins_parse_and_share_the_standard_layout() {
    let ids: Vec<&str> = BUILTINS.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [
            "general",
            "standup",
            "one-on-one",
            "client-call",
            "interview",
            "lecture"
        ]
    );
    let all = templates::builtins();
    assert_eq!(all.len(), 6);
    for (t, (id, _)) in all.iter().zip(BUILTINS) {
        assert_eq!(t.id, id);
        assert!(!t.name.is_empty() && !t.description.is_empty() && !t.sections.is_empty());
        assert_eq!(t.layout, all[0].layout, "{id}: the standard layout");
    }
    assert_eq!(all[0].name, "General meeting");
    let interview = &all[4].instructions;
    assert!(
        interview.contains("do not recommend hiring"),
        "no hiring recommendation"
    );
}

#[test]
fn the_standard_layout_renders_every_variable() {
    // F11-AC6 / the F11 DoD: title, summary, decisions, sections, to-dos with [ ]/[x], markers.
    // The spec's layout text as is: its block tags leave a blank line here and there.
    for t in templates::builtins() {
        let out = render(&t.layout, &sample_context()).unwrap();
        let want = "# Sample: weekly planning\nWednesday 2026-10-07 · 21:17 · 32 min · Sample\n\n\
                    ## Summary\nThe team agreed the Q4 scope and who sends the deck.\n\n\
                    ## Decisions\n- Ship the reader before the editor\n\n\n\
                    ## Blockers\n- None raised.\n\n\
                    ## To-dos\n- [ ] Send the deck to Priya — me (due 2026-10-09)\n\
                    - [x] Book the review room\n\n\
                    ## Transcript\n[0:00:00] Let's start with the plan.\n\
                    [0:15:00] I'll send the deck to Priya by Friday.\n\n";
        assert_eq!(out, want, "{}", t.id);
    }
    // Untitled, no decisions.
    let mut c = sample_context();
    c.title = None;
    c.decisions.clear();
    let out = render(&templates::builtins()[0].layout, &c).unwrap();
    assert!(out.starts_with("# Untitled meeting\n"));
    assert!(!out.contains("## Decisions"));
}

#[test]
fn templates_round_trip_through_the_file_format() {
    for t in templates::builtins() {
        assert_eq!(parse(&to_file(&t)).unwrap(), t, "{}", t.id);
    }
    let mut t = templates::builtins().remove(1);
    t.name = "\"Quoted\" stand-up".into();
    t.description = " leading space: and a colon".into();
    t.vocabulary = vec!["Priya".into(), "'Arjun'".into()];
    let file = to_file(&t);
    assert!(file.contains("vocabulary: [Priya, \"'Arjun'\"]"), "{file}");
    assert_eq!(parse(&file).unwrap(), t);
    // Instructions can't smuggle in a second layout heading.
    t.instructions = "Do it.\n# Layout\n{{ secret }}".into();
    assert_eq!(
        templates::validate(&t).unwrap_err().field,
        Some("instructions")
    );
}
