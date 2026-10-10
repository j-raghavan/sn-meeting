//! F20-FR8 / F20-AC5: a tapped circle lists exactly that tag's recordings, newest first, each
//! with its other tags.
mod libcommon;

use chrono::NaiveDate;
use libcommon::*;
use meeting::app::App;
use meeting::sidecar::{Tag, TagSource};
use std::path::PathBuf;

const HOUR: i64 = 3_600_000;

fn meeting(app: &App, at: i64, title: &str, tags: &[&str]) -> String {
    let b = begin(app, at);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 90_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut sc = read_sidecar(&dir, &id);
    sc.title = Some(title.into());
    sc.tags = tags.iter().map(|t| Tag::new(t, TagSource::Ai)).collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    id
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 9).unwrap()
}

#[test]
fn a_tag_lists_its_recordings_newest_first() {
    let e = Env::new();
    let app = e.app();
    // Recorded out of order, with one untagged and one other-tag meeting between them.
    let order = [3, 0, 4, 1, 2];
    let mut ids = vec![String::new(); 5];
    for i in order {
        let tags: &[&str] = if i == 4 {
            &["gpu", "local-ai", "microsoft"]
        } else {
            &["gpu"]
        };
        ids[i] = meeting(&app, T0 + i as i64 * HOUR, &format!("Talk {i}"), tags);
    }
    meeting(&app, T0 + 5 * HOUR, "Budget", &["budget"]);
    meeting(&app, T0 + 6 * HOUR, "Nothing", &[]);
    let r = parse(&app.tag_meetings_at("#GPU", "all", today()));
    assert_eq!(r["tag"], "gpu");
    assert_eq!(r["count"], 5);
    let got: Vec<&str> = r["meetings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    let want: Vec<&str> = ids.iter().rev().map(String::as_str).collect();
    assert_eq!(got, want, "newest to oldest");
    let first = &r["meetings"][0];
    assert_eq!(first["title"], "Talk 4");
    assert_eq!(first["tags"], serde_json::json!(["local-ai", "microsoft"]));
    assert_eq!(first["durationMs"], 90_000);
    let ms: Vec<i64> = r["meetings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["startedAtMs"].as_i64().unwrap())
        .collect();
    assert!(ms.windows(2).all(|w| w[0] > w[1]));
    assert_eq!(
        parse(&app.tag_meetings_at("vram", "all", today()))["count"],
        0
    );
    assert!(
        parse(&app.tag_meetings_at("", "all", today()))
            .get("error")
            .is_some()
    );
    assert!(
        parse(&app.tag_meetings_at("gpu", "x", today()))
            .get("error")
            .is_some()
    );
    assert_eq!(parse(&app.tag_meetings_json("gpu", "3m"))["tag"], "gpu");
}
