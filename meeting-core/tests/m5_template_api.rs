//! F11-FR5: list, get, save, reset and preview through the app, as the template screens use
//! them (F11-AC1..AC4, AC6).
mod libcommon;

use libcommon::*;
use meeting::templates::{self, USER_DIR};
use serde_json::{Value, json};

fn standup_json(app: &meeting::app::App) -> Value {
    parse(&app.get_template_json("standup"))
}

#[test]
fn a_fresh_install_lists_six_builtins() {
    // F11-AC1.
    let e = Env::new();
    let app = e.app();
    let rows = parse(&app.list_templates_json());
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for r in rows {
        assert_eq!(
            (
                r["origin"].as_str(),
                r["builtin"].as_bool(),
                r["customised"].as_bool()
            ),
            (Some("builtin"), Some(true), Some(false))
        );
        assert!(r["problem"].is_null() && r["path"].is_null());
    }
    assert_eq!(rows[0]["id"], "general");
    assert_eq!(rows[0]["name"], "General meeting");
    assert_eq!(
        parse(&app.get_settings_json())["settings"]["defaultTemplate"],
        "general"
    );
    assert_eq!(
        parse(&app.get_template_json("nope")),
        json!({"error": "template not found"})
    );
}

#[test]
fn saving_writes_the_file_and_a_desktop_edit_comes_back() {
    // F11-AC2.
    let e = Env::new();
    let app = e.app();
    let mut t = standup_json(&app);
    t["instructions"] = json!("  Changed in the app.  ");
    let saved = parse(&app.save_template_json(&t.to_string()));
    assert_eq!(saved["origin"], "customised", "{saved}");
    assert_eq!(saved["instructions"], "Changed in the app.");
    let path = e.root.path().join(USER_DIR).join("standup.md");
    assert_eq!(saved["path"], path.to_string_lossy().as_ref());
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(
        file.contains("# Instructions\nChanged in the app.\n# Layout\n"),
        "{file}"
    );
    std::fs::write(
        &path,
        file.replace("Changed in the app.", "Edited on the desktop."),
    )
    .unwrap();
    assert_eq!(standup_json(&app)["instructions"], "Edited on the desktop.");
    // Duplicate: a save under a new id is YOURS.
    let mut dup = standup_json(&app);
    dup["id"] = json!("standup-copy");
    dup["name"] = json!("Stand-up (copy)");
    dup["vocabulary"] = json!([" Priya ", "Arjun"]);
    let r = parse(&app.save_template_json(&dup.to_string()));
    assert_eq!(
        (r["origin"].as_str(), r["builtin"].as_bool()),
        (Some("yours"), Some(false))
    );
    assert_eq!(r["vocabulary"], json!(["Priya", "Arjun"]));
}

#[test]
fn a_broken_layout_is_refused_with_its_line_and_nothing_is_written() {
    // F11-AC3.
    let e = Env::new();
    let app = e.app();
    let mut t = standup_json(&app);
    t["layout"] = json!("# {{ title }}\n{% for t in todos %}\n- {{ t.text }}\n");
    let r = parse(&app.save_template_json(&t.to_string()));
    assert_eq!(r["field"], "layout");
    let line = r["line"].as_u64().unwrap();
    assert!((2..=4).contains(&line), "{r}");
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("Template error on line")
    );
    assert!(!e.root.path().join(USER_DIR).exists());
    for (patch, field) in [
        (json!({"id": "Bad Id"}), "id"),
        (json!({"name": ""}), "name"),
        (json!({"instructions": ""}), "instructions"),
    ] {
        let mut t = standup_json(&app);
        for (k, v) in patch.as_object().unwrap() {
            t[k] = v.clone();
        }
        assert_eq!(
            parse(&app.save_template_json(&t.to_string()))["field"],
            field
        );
    }
    let r = parse(&app.save_template_json("[1]"));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("template must be a JSON object"),
        "{r}"
    );
}

#[test]
fn reset_restores_the_builtin_and_deletes_yours() {
    // F11-AC4, and reset of a YOURS template (M4/M5 rulings).
    let e = Env::new();
    let app = e.app();
    let mut t = standup_json(&app);
    t["instructions"] = json!("Changed.");
    app.save_template_json(&t.to_string());
    let dir = e.root.path().join(USER_DIR);
    assert!(dir.join("standup.md").exists());
    assert_eq!(
        parse(&app.reset_template_json("standup")),
        json!({"ok": true, "origin": "builtin"})
    );
    assert!(!dir.join("standup.md").exists());
    let back = standup_json(&app);
    assert_eq!(back["instructions"], templates::builtins()[1].instructions);
    t["id"] = json!("mine");
    app.save_template_json(&t.to_string());
    assert_eq!(
        parse(&app.reset_template_json("mine")),
        json!({"ok": true, "origin": null})
    );
    assert!(!dir.join("mine.md").exists());
    assert_eq!(
        parse(&app.reset_template_json("general")),
        json!({"ok": true, "origin": "builtin"})
    );
    assert_eq!(
        parse(&app.reset_template_json("../x")),
        json!({"error": "invalid template id"})
    );
    std::fs::create_dir_all(dir.join("dir-in-the-way.md")).unwrap();
    std::fs::write(dir.join("dir-in-the-way.md/x"), "x").unwrap();
    let r = parse(&app.reset_template_json("dir-in-the-way"));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("can't delete the template"),
        "{r}"
    );
}

#[test]
fn preview_renders_the_sample_meeting() {
    // F11-AC6.
    let e = Env::new();
    let app = e.app();
    let t = standup_json(&app);
    let md = parse(&app.preview_template_json(&t.to_string()))["markdown"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        md.starts_with(
            "# Sample: weekly planning\nWednesday 2026-10-07 · 21:17 · 32 min · Stand-up\n"
        ),
        "{md}"
    );
    for want in [
        "## Summary\nThe team agreed",
        "- [ ] Send the deck to Priya",
        "- [x] Book the review room",
        "[0:00:00] ",
        "[0:15:00] ",
    ] {
        assert!(md.contains(want), "{want}");
    }
    let r = parse(&app.preview_template_json(r#"{"layout": "ok\n{{ 1 + }}"}"#));
    assert_eq!(
        (r["line"].as_u64(), r["field"].as_str()),
        (Some(2), Some("layout"))
    );
    let r = parse(&app.preview_template_json(r#"{"layout": "{{ template }}"}"#));
    assert_eq!(r, json!({"markdown": "Sample"}));
    let big = json!({"layout": "x".repeat(templates::MAX_LAYOUT + 1)});
    assert_eq!(
        parse(&app.preview_template_json(&big.to_string()))["field"],
        "layout"
    );
    assert!(parse(&app.preview_template_json("nope"))["error"].is_string());
    assert!(
        !e.root.path().join(USER_DIR).exists(),
        "preview writes nothing"
    );
}

#[test]
fn a_save_that_cannot_be_written_is_an_error() {
    let e = Env::new();
    let app = e.app();
    std::fs::create_dir_all(e.root.path().join("MyStyle/Meetings")).unwrap();
    std::fs::write(e.root.path().join(USER_DIR), "a file where the folder goes").unwrap();
    let r = parse(&app.save_template_json(&standup_json(&app).to_string()));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("can't save the template"),
        "{r}"
    );
}

#[test]
fn a_differently_cased_file_is_not_saved_over() {
    // N4: on a case-insensitive folder (the device's shared storage, the Mac's temp dir)
    // standup.md and Standup.md are one file; a save would land in Standup.md, which the
    // scan doesn't read as a template.
    let e = Env::new();
    let app = e.app();
    let dir = e.root.path().join(USER_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Standup.md"), "the desktop's own notes").unwrap();
    let r = parse(&app.save_template_json(&standup_json(&app).to_string()));
    assert_eq!(
        r,
        json!({"error": "A file named Standup.md already exists", "line": null, "field": "id"})
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("Standup.md")).unwrap(),
        "the desktop's own notes"
    );
    // The same name in the same case is the user's copy, saved over as usual.
    std::fs::remove_file(dir.join("Standup.md")).unwrap();
    std::fs::write(dir.join("standup.md"), "old").unwrap();
    let r = parse(&app.save_template_json(&standup_json(&app).to_string()));
    assert_eq!(r["origin"], "customised", "{r}");
}
