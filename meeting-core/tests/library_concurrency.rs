//! F3-FR2 / INV3: sidecar writes from many threads (the CoreJobs lane, a job lane, any thread
//! calling the core) are serialised by one core lock, so no write is lost or torn.
mod libcommon;

use libcommon::*;
use serde_json::json;
use std::sync::Arc;

#[test]
fn parallel_todo_ticks_all_land() {
    let e = Env::new();
    let app = Arc::new(e.app());
    let id = begin(&app, T0)["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1000, "");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    // A big sidecar makes each read-modify-write slow enough to overlap.
    sc.summary = Some("x".repeat(3_000_000));
    sc.todos = (0..40)
        .map(|i| {
            serde_json::from_value(json!({"id": format!("t{i}"), "text": format!("todo {i}"),
                "owner": null, "due": null, "done": false, "firstSeen": ""}))
            .unwrap()
        })
        .collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    let handles: Vec<_> = (0..40)
        .map(|i| {
            let (app, id) = (app.clone(), id.clone());
            std::thread::spawn(move || app.set_todo_done_json(&id, &format!("t{i}"), true))
        })
        .collect();
    for h in handles {
        assert_eq!(parse(&h.join().unwrap()), json!({"ok": true}));
    }
    assert_eq!(parse(&app.open_todos_json()), json!([]));
    let sc = read_sidecar(&dir, &id);
    assert!(sc.todos.iter().all(|t| t.done));
    let tmps = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count();
    assert_eq!(tmps, 0);
}
