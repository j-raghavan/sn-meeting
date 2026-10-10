//! F20-FR5 / F20-AC6: tags are their own search unit. `#vram` matches only meetings tagged
//! `vram` (by prefix); plain `vram` finds the tag and the transcript alike; the Tags chip; and
//! the tags come back from the sidecars when the index is rebuilt (F20-FR3).
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::search::query::Clock;
use meeting::sidecar::{Segment, Tag, TagSource};
use serde_json::Value;
use std::path::PathBuf;

fn meeting(app: &App, at: i64, said: &str, tags: &[&str]) -> String {
    let b = begin(app, at);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 60_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut sc = read_sidecar(&dir, &id);
    sc.segments = vec![Segment {
        file: sc.segments[0].file.clone(),
        start_offset_ms: 0,
        duration_ms: Some(10_000),
        text: Some(said.into()),
        flag: None,
        extra: Default::default(),
    }];
    sc.tags = tags.iter().map(|t| Tag::new(t, TagSource::Ai)).collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    id
}

fn clock() -> Clock {
    Clock {
        now_ms: T0 + 86_400_000,
        today_start_ms: T0 + 86_400_000 - 3_600_000,
    }
}

/// `(meeting id, [hit kinds])` per group.
fn found(app: &App, q: &str, filter: &str) -> Vec<(String, Vec<String>)> {
    let r: Value = parse(&app.search_at(q, filter, 0, clock()));
    r["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("{r}"))
        .iter()
        .map(|g| {
            let kinds = g["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h["kind"].as_str().unwrap().to_owned())
                .collect();
            (g["id"].as_str().unwrap().to_owned(), kinds)
        })
        .collect()
}

#[test]
fn hash_terms_match_only_tags_and_plain_words_match_both() {
    let e = Env::new();
    let app = e.app();
    let tagged = meeting(&app, T0, "We talked about the budget.", &["vram", "gpu"]);
    let said = meeting(&app, T0 + 3_600_000, "The vram was too small.", &["budget"]);
    let other = meeting(&app, T0 + 7_200_000, "Nothing here.", &["local-ai"]);
    app.index_update_json();

    // F20-AC6: `#vram` finds only the meeting tagged vram, by its tag.
    assert_eq!(
        found(&app, "#vram", "all"),
        [(tagged.clone(), vec!["tag".into()])]
    );
    assert_eq!(
        found(&app, "#vr", "all"),
        [(tagged.clone(), vec!["tag".into()])]
    );
    // Plain `vram`: the tag and the transcript both.
    let both = found(&app, "vram", "all");
    let ids: Vec<&str> = both.iter().map(|(i, _)| i.as_str()).collect();
    assert_eq!(ids, [said.as_str(), tagged.as_str()]);
    assert_eq!(both[0].1, ["transcript"]);
    assert_eq!(both[1].1, ["tag"]);
    // The Tags chip keeps tag hits only.
    assert_eq!(
        found(&app, "vram", "tags"),
        [(tagged.clone(), vec!["tag".into()])]
    );
    // `#name` narrows; the other words then match inside those meetings.
    assert_eq!(
        found(&app, "#gpu budget", "all"),
        [(tagged.clone(), vec!["tag".into(), "transcript".into()])]
    );
    assert_eq!(
        found(&app, "#gpu budget", "transcript"),
        [(tagged.clone(), vec!["transcript".into()])]
    );
    assert!(found(&app, "#gpu #local-ai", "all").is_empty());
    assert_eq!(found(&app, "#local-ai", "all")[0].0, other);
    assert!(found(&app, "#!!", "all").is_empty());
    let r: Value = parse(&app.search_at("#vram", "all", 0, clock()));
    let hit = &r["groups"][0]["hits"][0];
    assert_eq!(hit["label"], "TAG");
    assert_eq!(hit["unitId"], "tag:vram");
    assert_eq!(hit["snippet"][0]["bold"], true);

    // The tags come back from the files when the index starts again.
    app.rebuild_index_json();
    assert_eq!(found(&app, "#vram", "all"), [(tagged, vec!["tag".into()])]);
}
