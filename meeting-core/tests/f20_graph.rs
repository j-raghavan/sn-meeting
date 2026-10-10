//! F20-FR7 / FR9: the Tag Graph from the meetings' tags, laid out by the core only when the
//! tags or counts change; All time or the last three months (F20-AC4 through the app).
mod libcommon;

use chrono::NaiveDate;
use libcommon::*;
use meeting::app::App;
use meeting::sidecar::{Tag, TagSource};
use std::path::PathBuf;

const DAY: i64 = 86_400_000;

fn meeting(app: &App, at: i64, tags: &[&str]) -> (String, PathBuf) {
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
    (id, dir)
}

/// 2026-10-09, two days after T0.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 9).unwrap()
}

#[test]
fn the_graph_sizes_circles_by_recordings_and_lays_out_once() {
    let e = Env::new();
    let app = e.app();
    let r = parse(&app.tag_graph_at("all", today()));
    assert_eq!(r["graph"]["nodes"], serde_json::json!([]), "no tags yet");
    for i in 0..5 {
        let tags: &[&str] = if i < 2 { &["gpu", "vram"] } else { &["gpu"] };
        meeting(&app, T0 + i * 3_600_000, tags);
    }
    meeting(&app, T0 + 6 * 3_600_000, &[]);
    let r = parse(&app.tag_graph_at("all", today()));
    assert_eq!(r["laidOut"], true);
    let g = &r["graph"];
    assert_eq!(g["meetings"], 5);
    let (gpu, vram) = (&g["nodes"][0], &g["nodes"][1]);
    assert_eq!(
        (gpu["name"].as_str(), gpu["count"].as_u64()),
        (Some("gpu"), Some(5))
    );
    assert_eq!(
        (vram["name"].as_str(), vram["count"].as_u64()),
        (Some("vram"), Some(2))
    );
    assert!(gpu["r"].as_f64() > vram["r"].as_f64());
    assert!(vram["r"].as_f64().unwrap() >= meeting::tags::graph::MIN_RATIO);
    assert_eq!(
        g["edges"],
        serde_json::json!([{"a": 0, "b": 1, "shared": 2, "weak": false}])
    );
    assert_eq!(g["bounds"].as_array().unwrap().len(), 4);
    // The same tags: the cached layout, not a new one.
    let again = parse(&app.tag_graph_at("all", today()));
    assert_eq!(again["laidOut"], false);
    assert_eq!(again["graph"], r["graph"]);
    meeting(&app, T0 + 8 * 3_600_000, &["vram"]);
    let changed = parse(&app.tag_graph_at("all", today()));
    assert_eq!(changed["laidOut"], true);
    assert_eq!(changed["graph"]["nodes"][1]["count"], 3);
    assert!(
        parse(&app.tag_graph_at("week", today()))
            .get("error")
            .is_some()
    );
    assert!(parse(&app.tag_graph_json("all")).get("graph").is_some());
}

#[test]
fn the_last_three_months_leave_older_meetings_out() {
    let e = Env::new();
    let app = e.app();
    meeting(&app, T0 - 120 * DAY, &["old-topic"]);
    meeting(&app, T0, &["new-topic"]);
    let names = |range: &str| -> Vec<String> {
        parse(&app.tag_graph_at(range, today()))["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names("all"), ["new-topic", "old-topic"]);
    assert_eq!(names("3m"), ["new-topic"]);
}

/// F20-AC7 (the graph side): after a rename into an existing tag there is one circle.
#[test]
fn a_merged_rename_leaves_one_circle() {
    let e = Env::new();
    let app = e.app();
    meeting(&app, T0, &["gpus"]);
    meeting(&app, T0 + 3_600_000, &["gpu"]);
    app.rename_tag_json("gpus", "gpu");
    let g = parse(&app.tag_graph_at("all", today()));
    let nodes = g["graph"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["count"], 2);
}
