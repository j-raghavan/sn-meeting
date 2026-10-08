//! F11-FR1 / FR4: the template file format, validation with file line numbers, and rendering
//! limits (missing variables empty, no include, fuel, output cap).
use meeting::templates::{self, Context, MAX_FILE_BYTES, parse, render, sample_context};

const GENERAL: &str = templates::BUILTINS[0].1;

/// A valid file with `layout` as its layout (which starts on line 11 with `FRONT`).
fn file(front: &str, layout: &str) -> String {
    format!("---\n{front}\n---\n# Instructions\nSummarise it.\n# Layout\n{layout}")
}

const FRONT: &str = "id: retro\nname: Retro\ndescription: d\nvocabulary: []\nsections: []";

#[test]
fn a_broken_layout_is_reported_on_its_file_line() {
    // F11-AC3: `{% for %}` without `endfor`.
    let src = file(
        FRONT,
        "# {{ title }}\n\n{% for t in todos %}- {{ t.text }}\n",
    );
    let e = parse(&src).unwrap_err();
    let line = e.line.unwrap();
    assert!(line >= 9, "file-relative: {e:?}");
    assert!(
        e.error
            .starts_with(&format!("Template error on line {line}: ")),
        "{e:?}"
    );
    assert_eq!(e.field, Some("layout"));
    let e = parse(&file(FRONT, "ok\n{{ 1 + }}\n")).unwrap_err();
    // Front matter on lines 1-7, headings and instructions on 8-10, the layout from 11.
    assert_eq!(e.line, Some(12));
    assert!(e.error.starts_with("Template error on line 12: "), "{e:?}");
}

#[test]
fn front_matter_is_flat_lenient_and_line_numbered() {
    let src = "---\r\nid: retro\r\nname: \"Retro: monthly\"\r\n# a comment\r\nowner: me\r\n\
               description: 'One line'\r\nvocabulary: [Priya, \"Arjun\", , Supernote]\r\n---\r\n\
               # Instructions\r\nDo it.\r\n# Layout\r\n{{ title }}\r\n";
    let t = parse(src).unwrap();
    assert_eq!(t.name, "Retro: monthly");
    assert_eq!(t.description, "One line");
    assert_eq!(t.vocabulary, ["Priya", "Arjun", "Supernote"]);
    assert!(t.sections.is_empty());
    assert_eq!(t.instructions, "Do it.");
    assert_eq!(
        t.layout, "{{ title }}\n",
        "everything after # Layout, verbatim"
    );
    let cases: [(&str, Option<usize>, &str); 9] = [
        ("", Some(1), "must start with"),
        ("---\nid: x\n", Some(1), "no closing"),
        ("---\nid: x\nno colon\n---\n", Some(3), "key: value"),
        ("---\nsections: a, b\n---\n", Some(2), "list like"),
        ("---\nid: x\n---\n# Layout\nx\n", None, "# Instructions"),
        ("---\nid: x\n---\n# Instructions\nx\n", None, "# Layout"),
        (
            "---\nid: x\n---\n# Layout\nx\n# Instructions\ny\n",
            Some(4),
            "must come before",
        ),
        (&file("id: Bad_Id\nname: x", ""), Some(2), "The id must be"),
        (
            &file("id: x\nname: x\n\nname: ", ""),
            Some(5),
            "The name must be",
        ),
    ];
    for (src, line, want) in cases {
        let e = parse(src).unwrap_err();
        assert_eq!(e.line, line, "{src:?}: {e:?}");
        assert!(e.error.contains(want), "{src:?}: {e:?}");
    }
    let big = file(FRONT, &"x".repeat(MAX_FILE_BYTES));
    assert_eq!(
        parse(&big).unwrap_err().error,
        "The file is larger than 64 KB"
    );
}

#[test]
fn every_field_is_limited() {
    let ok = parse(GENERAL).unwrap();
    let check = |f: &dyn Fn(&mut templates::Template), field: &str| {
        let mut t = ok.clone();
        f(&mut t);
        let e = templates::validate(&t).unwrap_err();
        assert_eq!(e.field, Some(field), "{e:?}");
    };
    check(&|t| t.id = "a--b".into(), "id");
    check(&|t| t.id = "-a".into(), "id");
    check(&|t| t.id = "x".repeat(65), "id");
    check(&|t| t.name = " ".into(), "name");
    check(&|t| t.name = "n".repeat(61), "name");
    check(&|t| t.name = "a\nb".into(), "name");
    check(&|t| t.description = "d".repeat(201), "description");
    check(&|t| t.description = "a\tb".into(), "description");
    check(&|t| t.vocabulary = vec!["v".into(); 51], "vocabulary");
    check(&|t| t.vocabulary = vec!["v".repeat(61)], "vocabulary");
    check(&|t| t.sections = vec!["a, b".into()], "sections");
    check(&|t| t.sections = vec![" ".into()], "sections");
    check(&|t| t.instructions = "  ".into(), "instructions");
    check(&|t| t.instructions = "i".repeat(8001), "instructions");
    check(&|t| t.layout = "l".repeat(20_001), "layout");
    check(&|t| t.layout = "{% if %}".into(), "layout");
    let mut t = ok.clone();
    t.name = "n".repeat(60);
    t.instructions = "i".repeat(8000);
    t.vocabulary = vec!["v".repeat(60); 50];
    assert_eq!(templates::validate(&t), Ok(()));
}

#[test]
fn rendering_is_lenient_and_bounded() {
    let ctx = sample_context();
    assert_eq!(
        render("a{{ nope }}b{{ nope.deeper.still }}c\n", &ctx).unwrap(),
        "abc\n"
    );
    // A null value prints as nothing, like a missing one.
    let none = "[{{ title }}][{{ todos[1].owner }}]";
    assert_eq!(render(none, &Context::default()).unwrap(), "[][]");
    assert_eq!(render(none, &ctx).unwrap(), "[Sample: weekly planning][]");
    // No loader and no multi-template support: include, import and extends fail to compile.
    for tag in [
        "{% include 'secret.md' %}",
        "{% import 'x' as y %}",
        "{% extends 'base' %}",
    ] {
        let e = render(tag, &ctx).unwrap_err();
        assert!(e.message.starts_with("Template error"), "{tag}: {e:?}");
    }
    let runaway =
        "{% for a in range(100000) %}{% for b in range(100000) %}x{% endfor %}{% endfor %}";
    let e = render(runaway, &ctx).unwrap_err();
    assert!(e.message.starts_with("Template error"), "{e:?}");
    let mut big = sample_context();
    big.summary = "s".repeat(templates::MAX_OUTPUT / 2 + 1);
    let e = render("{{ summary }}{{ summary }}", &big).unwrap_err();
    assert_eq!(e.message, "Template error: the output is larger than 2 MB");
    // Markdown, not HTML: no escaping.
    let mut c = sample_context();
    c.summary = "<b>a & b</b>".into();
    assert_eq!(render("{{ summary }}", &c).unwrap(), "<b>a & b</b>");
}
