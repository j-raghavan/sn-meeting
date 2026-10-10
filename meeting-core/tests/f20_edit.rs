//! F20-FR4: the user adds and removes a meeting's tags, and renames a tag everywhere: every
//! sidecar in the save folders, merged where the new name exists (F20-AC7), one atomic write
//! at a time, and finished by the next call when it was cut short.
mod libcommon;

use libcommon::*;
use meeting::sidecar::{Tag, TagSource};
use std::path::PathBuf;

fn meeting(app: &meeting::app::App, at: i64, tags: &[&str]) -> (String, PathBuf) {
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

fn names(dir: &std::path::Path, id: &str) -> Vec<String> {
    read_sidecar(dir, id)
        .tags
        .into_iter()
        .map(|t| t.name)
        .collect()
}

#[test]
fn the_user_adds_and_removes_tags() {
    let e = Env::new();
    let app = e.app();
    let (id, dir) = meeting(&app, T0, &["gpu"]);
    let r = parse(&app.add_tag_json(&id, "  #Local AI "));
    assert_eq!(
        r["tags"],
        serde_json::json!([{"name": "gpu", "source": "ai"}, {"name": "local-ai", "source": "user"}])
    );
    let r = parse(&app.remove_tag_json(&id, "#GPU"));
    assert_eq!(r["tags"].as_array().unwrap().len(), 1);
    let sc = read_sidecar(&dir, &id);
    assert_eq!(sc.tags_user_removed, ["gpu"]);
    assert!(parse(&app.add_tag_json(&id, "!!")).get("error").is_some());
    assert!(parse(&app.remove_tag_json(&id, "")).get("error").is_some());
    assert!(
        parse(&app.add_tag_json("meeting-20990101-000000", "x"))
            .get("error")
            .is_some()
    );
}

/// F20-AC7.
#[test]
fn a_rename_merges_into_an_existing_tag_everywhere() {
    let e = Env::new();
    let app = e.app();
    let (a, dir) = meeting(&app, T0, &["gpus", "gpu"]);
    let (b, _) = meeting(&app, T0 + 3_600_000, &["gpus", "vram"]);
    let (c, _) = meeting(&app, T0 + 7_200_000, &["budget"]);
    let before = std::fs::metadata(dir.join(format!("{c}.json")))
        .unwrap()
        .modified()
        .unwrap();
    let r = parse(&app.rename_tag_json("GPUs", "gpu"));
    assert_eq!(r["renamed"], 2);
    assert_eq!(r["to"], "gpu");
    assert_eq!(names(&dir, &a), ["gpu"]);
    assert_eq!(names(&dir, &b), ["gpu", "vram"]);
    assert_eq!(names(&dir, &c), ["budget"]);
    let after = std::fs::metadata(dir.join(format!("{c}.json")))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after, "a meeting without the tag isn't rewritten");
    assert!(
        read_sidecar(&dir, &b)
            .tags_user_removed
            .contains(&"gpus".into())
    );
    assert!(!e.data.path().join("tag-rename.json").exists());
    assert!(
        parse(&app.rename_tag_json("x", "??"))
            .get("error")
            .is_some()
    );
}

#[test]
fn a_rename_cut_short_is_finished_later() {
    let e = Env::new();
    let app = e.app();
    let (a, dir) = meeting(&app, T0, &["q4"]);
    std::fs::write(
        e.data.path().join("tag-rename.json"),
        r#"{"from":"q4","to":"fourth-quarter"}"#,
    )
    .unwrap();
    assert_eq!(app.resume_tag_rename(), Some(1));
    assert_eq!(names(&dir, &a), ["fourth-quarter"]);
    assert_eq!(app.resume_tag_rename(), None, "nothing left to finish");
    std::fs::write(e.data.path().join("tag-rename.json"), b"junk").unwrap();
    assert_eq!(app.resume_tag_rename(), Some(0));
    assert!(!e.data.path().join("tag-rename.json").exists());
    // The next rename finishes one left behind first.
    std::fs::write(
        e.data.path().join("tag-rename.json"),
        r#"{"from":"fourth-quarter","to":"q4"}"#,
    )
    .unwrap();
    let r = parse(&app.rename_tag_json("q4", "budget"));
    assert_eq!(r["renamed"], 1);
    assert_eq!(names(&dir, &a), ["budget"]);
}
