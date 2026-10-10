//! F20-FR6: each recording row on Home carries up to three of its tags, the most used first.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::sidecar::{Tag, TagSource};
use std::path::PathBuf;

const HOUR: i64 = 3_600_000;

fn meeting(app: &App, at: i64, tags: &[&str]) -> String {
    let b = begin(app, at);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 60_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut sc = read_sidecar(&dir, &id);
    sc.tags = tags.iter().map(|t| Tag::new(t, TagSource::Ai)).collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    id
}

#[test]
fn home_rows_show_three_tags_most_used_first() {
    let e = Env::new();
    let app = e.app();
    // gpu used 3 times, vram twice, the rest once.
    let main = meeting(&app, T0, &["rtx-spark", "vram", "microsoft", "gpu"]);
    meeting(&app, T0 + HOUR, &["gpu", "vram"]);
    let plain = meeting(&app, T0 + 2 * HOUR, &[]);
    meeting(&app, T0 + 3 * HOUR, &["gpu"]);
    let a = app
        .agenda_at("2026-10-07", "2026-10-07", T0 + 4 * HOUR)
        .unwrap();
    let rows = a["days"][0]["rows"].as_array().unwrap();
    let row = |id: &str| rows.iter().find(|r| r["meetingId"] == id).unwrap();
    assert_eq!(
        row(&main)["tags"],
        serde_json::json!(["gpu", "vram", "rtx-spark"])
    );
    assert_eq!(row(&plain)["tags"], serde_json::json!([]));
}
