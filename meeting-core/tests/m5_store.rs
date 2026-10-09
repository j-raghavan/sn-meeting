//! F11-FR3: user copies override built-ins when valid; an invalid copy falls back with the
//! line; user-only templates are YOURS; the folder is rescanned when a file changes; no folder
//! means the six built-ins (ADR D10).
mod libcommon;

use libcommon::*;
use meeting::app::Job;
use meeting::templates::{self, Origin, Store, USER_DIR, to_file};
use std::path::Path;

fn standup() -> templates::Template {
    templates::builtins().remove(1)
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn no_folder_gives_the_six_builtins() {
    let e = Env::new();
    let s = Store::under(e.root.path());
    let l = s.list();
    assert_eq!(l.len(), 6);
    assert!(
        l.iter()
            .all(|x| x.origin == Origin::Builtin && x.problem.is_none() && x.path.is_none())
    );
    assert_eq!(s.effective(), templates::builtins());
    assert!(
        !e.root.path().join("MyStyle").exists(),
        "listing creates nothing"
    );
}

#[test]
fn user_copies_take_precedence_when_valid() {
    let e = Env::new();
    let dir = e.root.path().join(USER_DIR);
    let mut su = standup();
    su.instructions = "Changed on the desktop.".into();
    write(&dir, "standup.md", &to_file(&su));
    // F11-AC3: a broken copy from the desktop.
    let broken =
        to_file(&templates::builtins()[0]).replace("{% endfor %}{% endif %}", "{% endif %}");
    write(&dir, "general.md", &broken);
    let mut retro = standup();
    retro.id = "retro".into();
    retro.name = "Retro".into();
    write(&dir, "retro.md", &to_file(&retro));
    write(&dir, "other.md", &to_file(&retro)); // id doesn't match the file name
    write(&dir, "notes.txt", "not a template");
    write(&dir, "Bad_Name.md", &to_file(&retro));
    std::fs::create_dir_all(dir.join("sub.md")).unwrap();
    let s = Store::new(dir.clone());
    let l = s.list();
    let get = |id: &str| l.iter().find(|x| x.template.id == id).unwrap().clone();
    let st = get("standup");
    assert_eq!(
        (st.origin, st.template.instructions.as_str()),
        (Origin::Customised, "Changed on the desktop.")
    );
    assert_eq!(st.path.unwrap(), dir.join("standup.md").to_string_lossy());
    let g = get("general");
    assert_eq!(g.origin, Origin::Builtin);
    assert_eq!(g.template, templates::builtins()[0], "the built-in is used");
    let p = g.problem.unwrap();
    assert!(
        p.starts_with("Your copy has an error on line ") && p.ends_with("; using the built-in"),
        "{p}"
    );
    assert_eq!(get("retro").origin, Origin::Yours);
    let other = get("other");
    assert_eq!(
        other.problem.as_deref(),
        Some("This file has an error (its id retro doesn't match the file name); it can't be used")
    );
    assert!(!other.usable());
    let ids: Vec<String> = l.iter().map(|x| x.template.id.clone()).collect();
    assert_eq!(
        ids,
        [
            "general",
            "standup",
            "one-on-one",
            "client-call",
            "interview",
            "lecture",
            "other",
            "retro"
        ]
    );
    let eff: Vec<String> = s.effective().into_iter().map(|t| t.id).collect();
    assert_eq!(eff.len(), 7);
    assert!(!eff.contains(&"other".to_owned()));
    assert_eq!(s.get("retro").unwrap().template.name, "Retro");
    assert!(s.get("missing").is_none());
}

#[test]
fn unreadable_and_oversized_files_fall_back() {
    let e = Env::new();
    let dir = e.root.path().join(USER_DIR);
    write(
        &dir,
        "standup.md",
        &"x".repeat(templates::MAX_FILE_BYTES + 1),
    );
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lecture.md"), [0xff, 0xfe, 0x00]).unwrap();
    let s = Store::new(dir);
    let p = |id: &str| s.get(id).unwrap().problem.unwrap();
    assert_eq!(
        p("standup"),
        "Your copy has an error (the file is larger than 64 KB); using the built-in"
    );
    assert_eq!(
        p("lecture"),
        "Your copy has an error (the file isn't UTF-8 text); using the built-in"
    );
}

#[test]
fn files_are_rescanned_only_when_they_change() {
    // F11-AC2 (desktop edit reaches the app) and the fingerprint cache.
    let e = Env::new();
    let dir = e.root.path().join(USER_DIR);
    let s = Store::new(dir.clone());
    s.list();
    s.list();
    assert_eq!(s.rebuilds(), 1, "unchanged: parsed once");
    let mut su = standup();
    su.instructions = "First edit.".into();
    write(&dir, "standup.md", &to_file(&su));
    assert_eq!(
        s.get("standup").unwrap().template.instructions,
        "First edit."
    );
    su.instructions = "Second edit, longer.".into();
    write(&dir, "standup.md", &to_file(&su));
    assert_eq!(
        s.get("standup").unwrap().template.instructions,
        "Second edit, longer."
    );
    assert_eq!(s.rebuilds(), 3);
    std::fs::remove_file(dir.join("standup.md")).unwrap();
    assert_eq!(s.get("standup").unwrap().origin, Origin::Builtin);
}

#[test]
fn a_job_uses_the_effective_set_taken_at_its_start() {
    let e = Env::new();
    let app = e.app();
    let keys = e.data.path().join("no-keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(
        keys.join("copilot-key-openai.txt"),
        "provider=openai\nkey=sk-x\nmodel=m\n",
    )
    .unwrap();
    let mut su = standup();
    su.instructions = "Customised.".into();
    write(&e.root.path().join(USER_DIR), "standup.md", &to_file(&su));
    let plan = app.plan(Job::Summarise).unwrap();
    let t = plan.templates.iter().find(|t| t.id == "standup").unwrap();
    assert_eq!(t.instructions, "Customised.");
    assert_eq!(app.templates.dir(), e.root.path().join(USER_DIR));
}

#[test]
fn a_bom_user_copy_is_used() {
    // Desktop editors may save with a UTF-8 BOM, or with old-Mac lone-CR line endings.
    let e = Env::new();
    let dir = e.root.path().join(USER_DIR);
    let mut su = standup();
    su.instructions = "Saved with a BOM.".into();
    write(&dir, "standup.md", &format!("\u{feff}{}", to_file(&su)));
    let mut retro = standup();
    retro.id = "retro".into();
    retro.name = "Retro".into();
    write(&dir, "retro.md", &to_file(&retro).replace('\n', "\r"));
    let l = Store::new(dir).list();
    let get = |id: &str| l.iter().find(|x| x.template.id == id).unwrap().clone();
    let st = get("standup");
    assert_eq!(
        (st.origin, st.problem.clone()),
        (Origin::Customised, None),
        "{st:?}"
    );
    assert_eq!(st.template.instructions, "Saved with a BOM.");
    let r = get("retro");
    assert_eq!(
        (r.origin, r.problem.clone()),
        (Origin::Yours, None),
        "{r:?}"
    );
    assert_eq!(r.template.layout, standup().layout);
}
