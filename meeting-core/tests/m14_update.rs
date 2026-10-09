//! F19-FR5 / F19-AC3 / EC29: the index follows the files. Only changed meetings and calendar
//! links are re-indexed; deleted ones drop out; a missing, corrupt or older index is rebuilt
//! from the sidecars, notes text and event cache alone.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::search::Kind;
use meeting::search::query::{fts_query, matching};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A finished meeting at `at_ms` whose summary is `summary`; returns its id and folder.
fn meeting(app: &App, at_ms: i64, summary: &str) -> (String, PathBuf) {
    let b = begin(app, at_ms);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    app.finish_json(&id, 1_000, "");
    let mut sc = read_sidecar(&dir, &id);
    sc.summary = Some(summary.into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    (id, dir)
}

fn notes(dir: &Path, id: &str, text: &str) {
    let p = meeting::notes_text::path(dir, id);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let doc = json!([{"region": 0, "rect": [0, 0, 10, 10],
        "lines": [{"text": text, "box": [0, 0, 1, 1], "strokeIds": [1], "atMs": 5}]}]);
    std::fs::write(p, doc.to_string()).unwrap();
}

fn events(app: &App, link: &str, list: Value) {
    let d = app.calendar_dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join(format!("{link}.json")),
        json!({"events": list}).to_string(),
    )
    .unwrap();
}

fn event(link: &str, uid: &str, title: &str) -> Value {
    json!({"eventKey": format!("{link}:{uid}:-"), "title": title,
           "start": "2026-10-09T10:00:00-07:00", "attendees": [{"name": "Priya"}]})
}

fn update(app: &App) -> Value {
    parse(&app.index_update_json())
}

/// The groups matching `q`, sorted.
fn groups(app: &App, q: &str) -> Vec<String> {
    let fts = fts_query(q).unwrap();
    let mut g: Vec<String> = app
        .search
        .with(|ix| matching(ix, &fts, None).unwrap())
        .into_iter()
        .map(|m| m.group)
        .collect();
    g.sort();
    g.dedup();
    g
}

#[test]
fn only_what_changed_is_indexed_again() {
    let e = Env::new();
    let app = e.app();
    assert_eq!(parse(&app.index_status_json())["state"], "missing");
    let (a, dir) = meeting(&app, T0, "Budget talk");
    let (b, _) = meeting(&app, T0 + 60_000, "Hiring");
    notes(&dir, &a, "Q3 budget");
    events(
        &app,
        "l1",
        json!([event("l1", "u1", "Budget sync"), event("l1", "u2", "Lunch")]),
    );
    let u = update(&app);
    assert_eq!(u, json!({"indexed": 4, "removed": 0, "rebuilt": false}));
    assert_eq!(
        parse(&app.index_status_json()),
        json!({"state": "ready", "done": 3, "total": 3}),
        "meetings and links"
    );
    assert_eq!(
        groups(&app, "budget"),
        ["e:l1:u1:-".to_owned(), format!("m:{a}")]
    );
    assert_eq!(
        app.search.with(|ix| ix.count(Some(Kind::Notes)).unwrap()),
        1
    );
    assert_eq!(update(&app)["indexed"], 0, "nothing changed");

    // A rename is a sidecar write: that meeting only.
    std::thread::sleep(std::time::Duration::from_millis(5));
    app.rename_json(&b, "Budget hiring");
    let mut sc = read_sidecar(&dir, &b);
    sc.summary = Some("Budget hiring plan".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    assert_eq!(update(&app)["indexed"], 1);
    assert_eq!(groups(&app, "hiring"), [format!("m:{b}")]);
    let g = app
        .search
        .with(|ix| ix.group(&format!("m:{b}")).unwrap().unwrap());
    assert_eq!(g.title.as_deref(), Some("Budget hiring"));

    // Notes text changed: that meeting again.
    notes(&dir, &a, "Q4 plans");
    assert_eq!(update(&app)["indexed"], 1);
    assert_eq!(groups(&app, "q4"), [format!("m:{a}")]);
    assert!(groups(&app, "q3").is_empty());

    // An event file changed: its link's events are replaced.
    events(&app, "l1", json!([event("l1", "u2", "Lunch budget")]));
    let u = update(&app);
    assert_eq!(
        (u["indexed"].as_u64(), u["removed"].as_u64()),
        (Some(1), Some(2))
    );
    assert_eq!(
        groups(&app, "budget"),
        ["e:l1:u2:-".to_owned(), format!("m:{a}"), format!("m:{b}")]
    );
    assert_eq!(groups(&app, "priya"), ["e:l1:u2:-"]);

    // Deleted on the desktop (D5), and a calendar link gone: dropped at the next scan.
    std::fs::remove_file(dir.join(format!("{b}.json"))).unwrap();
    std::fs::remove_file(app.calendar_dir().join("l1.json")).unwrap();
    let u = update(&app);
    assert_eq!(
        (u["indexed"].as_u64(), u["removed"].as_u64()),
        (Some(0), Some(2))
    );
    assert!(groups(&app, "lunch").is_empty());
}

#[test]
fn unreadable_files_are_not_searchable() {
    let e = Env::new();
    let app = e.app();
    let (a, dir) = meeting(&app, T0, "Budget");
    update(&app);
    std::fs::write(dir.join(format!("{a}.json")), "{ not json").unwrap();
    assert_eq!(update(&app)["indexed"], 0);
    assert!(groups(&app, "budget").is_empty());
    // Legacy audio has no text; a corrupt notes file is no notes.
    std::fs::write(dir.join("meeting-20261001-090000-01.m4a"), b"x").unwrap();
    let (c, _) = meeting(&app, T0 + 120_000, "Roadmap");
    std::fs::create_dir_all(dir.join(format!("{c}.notes"))).unwrap();
    std::fs::write(meeting::notes_text::path(&dir, &c), "[").unwrap();
    assert_eq!(update(&app)["indexed"], 1);
    assert_eq!(groups(&app, "roadmap"), [format!("m:{c}")]);
    // Event files: junk, the wrong link's events and odd names are ignored.
    let cal = app.calendar_dir();
    events(
        &app,
        "l2",
        json!([event("other", "u", "Budget"), {"eventKey": 3}]),
    );
    std::fs::write(cal.join("l3.json"), "junk").unwrap();
    std::fs::write(
        cal.join("bad-name.json"),
        json!({"events": [event("bad-name", "u", "Budget")]}).to_string(),
    )
    .unwrap();
    std::fs::write(cal.join("notes.txt"), "x").unwrap();
    std::fs::write(cal.join(".json"), "x").unwrap();
    assert_eq!(update(&app)["indexed"], 0);
    assert!(groups(&app, "budget").is_empty());
}

/// F19-AC3 / EC29: a deleted, corrupt or older index is rebuilt from the files, with the same
/// results and no provider call (no key exists in this environment).
#[test]
fn a_lost_or_broken_index_is_rebuilt_from_the_files() {
    let e = Env::new();
    let app = e.app();
    let (a, dir) = meeting(&app, T0, "Budget talk");
    notes(&dir, &a, "Q3 budget");
    events(&app, "l1", json!([event("l1", "u1", "Budget sync")]));
    update(&app);
    let before = groups(&app, "budget");
    let file = e.data.path().join("search/index.sqlite");
    assert!(file.is_file(), "app-private, under the app data dir");

    // Deleted while the app was away: a new process (a new App) starts from nothing.
    std::fs::remove_file(&file).unwrap();
    let app2 = e.app();
    assert_eq!(parse(&app2.index_status_json())["state"], "missing");
    assert_eq!(update(&app2)["indexed"], 2);
    assert_eq!(groups(&app2, "budget"), before);
    drop(app2);

    // Corrupt.
    std::fs::write(&file, vec![7u8; 4096]).unwrap();
    let app3 = e.app();
    assert_eq!(parse(&app3.index_status_json())["state"], "missing");
    update(&app3);
    assert_eq!(groups(&app3, "budget"), before);
    drop(app3);

    // An older index version.
    let c = rusqlite_version_bump(&file);
    assert!(c);
    let app4 = e.app();
    update(&app4);
    assert_eq!(groups(&app4, "budget"), before);

    // Settings → Rebuild search index.
    let r = parse(&app4.rebuild_index_json());
    assert_eq!(r, json!({"indexed": 2, "removed": 0, "rebuilt": true}));
    assert_eq!(groups(&app4, "budget"), before);
    assert_eq!(parse(&app4.index_status_json())["state"], "ready");
}

/// Set the index's `user_version` to 0 with a table present: an index from another version.
fn rusqlite_version_bump(file: &Path) -> bool {
    // Overwrite the header's user_version (offset 60, big-endian u32) directly.
    let mut b = std::fs::read(file).unwrap();
    b[60..64].copy_from_slice(&999u32.to_be_bytes());
    std::fs::write(file, b).is_ok()
}

#[test]
fn storage_that_cant_hold_the_index_falls_back_to_memory() {
    let e = Env::new();
    // The search folder's place is taken by a file.
    std::fs::write(e.data.path().join("search"), b"x").unwrap();
    let app = e.app();
    meeting(&app, T0, "Budget");
    assert_eq!(update(&app)["indexed"], 1);
    assert_eq!(groups(&app, "budget").len(), 1);
}
