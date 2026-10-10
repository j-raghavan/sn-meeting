//! F20-FR3: tags live in the sidecar as optional schema-1 fields: `tags: [{name, source}]` and
//! `tagsUserRemoved: [name]`. Absent means none and nothing is written for none; what a newer
//! build adds inside a tag survives a rewrite; a hand-edited entry that isn't a tag is dropped
//! instead of making the meeting unreadable.
use meeting::sidecar::{self, Tag, TagSource};
use serde_json::{Value, json};

const ID: &str = "meeting-20261007-211706";

fn base() -> Value {
    json!({"schema": 1, "id": ID, "startedAt": "2026-10-07T21:17:06-07:00", "status": "done"})
}

#[test]
fn tags_round_trip_through_the_file() {
    let d = tempfile::tempdir().unwrap();
    let mut sc = sidecar::blank(ID);
    sc.started_at = "2026-10-07T21:17:06-07:00".into();
    sc.tags = vec![
        Tag::new("gpu", TagSource::Ai),
        Tag::new("budget", TagSource::User),
    ];
    sc.tags_user_removed = vec!["q4".into()];
    sidecar::write(d.path(), &sc).unwrap();
    let raw: Value =
        serde_json::from_slice(&std::fs::read(d.path().join(format!("{ID}.json"))).unwrap())
            .unwrap();
    assert_eq!(
        raw["tags"],
        json!([{"name": "gpu", "source": "ai"}, {"name": "budget", "source": "user"}])
    );
    assert_eq!(raw["tagsUserRemoved"], json!(["q4"]));
    assert_eq!(sidecar::read(d.path(), ID).unwrap(), sc);
}

#[test]
fn no_tags_writes_nothing_and_an_old_sidecar_reads() {
    let sc: sidecar::Sidecar = serde_json::from_value(base()).unwrap();
    assert!(sc.tags.is_empty() && sc.tags_user_removed.is_empty());
    let out = serde_json::to_value(&sc).unwrap();
    assert!(out.get("tags").is_none() && out.get("tagsUserRemoved").is_none());
}

#[test]
fn unknown_fields_in_a_tag_survive_and_bad_entries_are_dropped() {
    let mut v = base();
    v["tags"] = json!([
        {"name": "GPU", "source": "ai", "weight": 3},
        {"name": "gpu", "source": "user"},
        {"name": "!!!", "source": "user"},
        {"name": "vram", "source": "robot"},
        "loose",
        {"name": "Local AI", "source": "user"}
    ]);
    v["tagsUserRemoved"] = json!(["Budget", 7, "budget", "#q4"]);
    let sc: sidecar::Sidecar = serde_json::from_value(v).unwrap();
    let names: Vec<(&str, TagSource)> = sc
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.source))
        .collect();
    assert_eq!(
        names,
        [("gpu", TagSource::Ai), ("local-ai", TagSource::User)]
    );
    assert_eq!(sc.tags[0].extra["weight"], 3);
    assert_eq!(sc.tags_user_removed, ["budget", "q4"]);
    let out = serde_json::to_value(&sc).unwrap();
    assert_eq!(
        out["tags"][0],
        json!({"name": "gpu", "source": "ai", "weight": 3})
    );
}

#[test]
fn a_tags_field_that_is_not_a_list_reads_as_none() {
    let mut v = base();
    v["tags"] = json!("gpu");
    v["tagsUserRemoved"] = json!(null);
    let sc: sidecar::Sidecar = serde_json::from_value(v).unwrap();
    assert!(sc.tags.is_empty() && sc.tags_user_removed.is_empty());
}
