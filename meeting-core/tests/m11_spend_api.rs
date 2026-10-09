//! F16-FR5 / F16-AC5 / F16-AC6 through `App`: a meeting's cost on its Home row and in
//! nativeGetMeeting, and nativeSpend's months, from the usage records in the sidecars.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::keyfile::Provider;
use meeting::providers::Measured;
use meeting::rates::RateTable;
use meeting::spend::{Kind, UsageEntry, UsageRecord};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A meeting started at `at_ms` with `records` added to its sidecar.
fn meeting_with(app: &App, at_ms: i64, records: Vec<UsageRecord>) -> (String, PathBuf) {
    let b = begin(app, at_ms);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    app.finish_json(&id, 1_000, "");
    let mut sc = read_sidecar(&dir, &id);
    sc.usage = records.into_iter().map(UsageEntry::from).collect();
    meeting::sidecar::write(&dir, &sc).unwrap();
    (id, dir)
}

fn tokens(model: &str, at: &str, input: u64) -> UsageRecord {
    let m = Measured {
        input_tokens: Some(input),
        output_tokens: Some(0),
        ..Measured::new(Provider::OpenAi, model)
    };
    UsageRecord::new(Kind::Summarise, &m, at.into(), &RateTable::default())
}

fn row<'a>(list: &'a Value, id: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == id)
        .unwrap()
}

/// F16-AC5: a record with no rate shows "+"; a meeting with none shows "—".
#[test]
fn rows_and_the_meeting_show_its_cost() {
    let e = Env::new();
    let app = e.app();
    let at = "2026-10-07T22:00:00-07:00";
    // 2 733 334 gpt-4o-mini input tokens at $0.15 per 1M ≈ $0.41.
    let priced = tokens("gpt-4o-mini", at, 2_733_334);
    let unpriced = tokens("gpt-9", at, 1_000);
    assert_eq!(unpriced.usd, None);
    let (a, _) = meeting_with(&app, T0, vec![priced, unpriced]);
    let (b, _) = meeting_with(&app, T0 + 60_000, vec![]);
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $0.41+");
    assert_eq!(row(&list, &a)["spend"]["unpriced"], 1);
    assert_eq!(row(&list, &b)["spend"]["label"], "—");
    let m = parse(&app.get_meeting_json(&a));
    assert_eq!(m["spend"]["label"], "≈ $0.41+");
    assert_eq!(m["meeting"]["usage"].as_array().unwrap().len(), 2);
}

/// F16-AC6: Home in October counts the October record only.
#[test]
fn months_follow_local_time() {
    let e = Env::new();
    let app = e.app(); // UTC-7
    let sep = tokens("gpt-4o-mini", "2026-09-30T23:50:00-07:00", 1_000_000);
    let oct = tokens("gpt-4o-mini", "2026-10-01T00:10:00-07:00", 2_000_000);
    meeting_with(&app, T0, vec![sep]);
    meeting_with(&app, T0 + 60_000, vec![oct]);
    let r = parse(&app.spend_json("2026-09", "2026-10"));
    assert_eq!(r["months"][0]["month"], "2026-09");
    assert_eq!(r["months"][0]["meetings"], 1);
    assert_eq!(r["months"][0]["usd"], 0.15);
    let october = &r["months"][1];
    assert_eq!(october["meetings"], 1);
    assert_eq!(october["usd"], 0.3);
    assert_eq!(october["label"], "October · $0.30 spent · 1 meeting");
    assert_eq!(
        october["byProvider"]["openai"],
        json!({"transcribe": 0.0, "summarise": 0.3})
    );
    let home = parse(&app.spend_json("2026-11", "2026-11"));
    assert_eq!(home["months"][0]["label"], "November · nothing spent yet");
    assert_eq!(
        parse(&app.spend_json("2026-11", "x")),
        json!({"error": "months must be YYYY-MM"})
    );
}

fn set_rates(app: &App, v: Value) {
    let r = parse(&app.set_rates_json(&v.to_string()));
    assert!(r.get("error").is_none(), "{r}");
}

/// F16-AC5 (whole) and F16-AC7: a rate edit never changes a stored cost; "Re-price" does, after
/// a dry run that says what would change.
#[test]
fn a_rate_edit_changes_nothing_until_re_price() {
    let e = Env::new();
    let app = e.app();
    let at = "2026-10-07T22:00:00-07:00";
    let (a, dir) = meeting_with(
        &app,
        T0,
        vec![
            tokens("gpt-4o-mini", at, 2_000_000),
            tokens("gpt-9", at, 1_000_000),
        ],
    );
    let (b, _) = meeting_with(
        &app,
        T0 + 60_000,
        vec![tokens("gpt-4o-mini", at, 1_000_000)],
    );
    meeting_with(&app, T0 + 120_000, vec![]);
    let before = read_sidecar(&dir, &a);
    assert_eq!(before.usage[0].record().unwrap().usd, Some(0.3));
    set_rates(
        &app,
        json!([{"provider": "openai", "model": "gpt-4o-mini", "unit": "tokens", "input": 0.5,
                "output": 0.6},
               {"provider": "openai", "model": "gpt-9", "unit": "tokens", "input": 0.11,
                "output": 1}]),
    );
    // F16-AC7: the edit wrote nothing to the meetings.
    assert_eq!(read_sidecar(&dir, &a), before);
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $0.30+");
    let dry = parse(&app.reprice_json(true));
    assert_eq!(
        dry,
        json!({"meetings": 2, "written": 0, "oldUsd": 0.45, "newUsd": 1.61,
               "unpricedBefore": 1, "unpricedAfter": 0})
    );
    assert_eq!(read_sidecar(&dir, &a), before, "a dry run writes nothing");
    let done = parse(&app.reprice_json(false));
    assert_eq!(done["written"], 2);
    let after = read_sidecar(&dir, &a);
    assert_eq!(after.usage[0].record().unwrap().usd, Some(1.0));
    assert_eq!(after.usage[1].record().unwrap().usd, Some(0.11));
    assert_eq!(
        after.usage[0].record().unwrap().input_tokens,
        before.usage[0].record().unwrap().input_tokens
    );
    assert_eq!(
        after.usage[0].record().unwrap().id,
        before.usage[0].record().unwrap().id
    );
    assert_eq!(
        after.usage[0]
            .record()
            .unwrap()
            .rate
            .as_ref()
            .unwrap()
            .as_of,
        app.today().format("%Y-%m-%d").to_string()
    );
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $1.11", "a full total");
    assert_eq!(row(&list, &b)["spend"]["label"], "≈ $0.50");
    // Nothing left to change.
    assert_eq!(parse(&app.reprice_json(false))["meetings"], 0);
}

/// F16-FR7: spend comes from the sidecars this device lists, in the current and previous save
/// folders; a sidecar copied in from another device counts, one outside them doesn't.
#[test]
fn spend_is_what_this_device_lists() {
    let e = Env::new();
    let app = e.app();
    let at = "2026-10-07T22:00:00-07:00";
    let (old, old_dir) = meeting_with(&app, T0, vec![tokens("gpt-4o-mini", at, 1_000_000)]);
    let r = parse(&app.set_settings_json(r#"{"saveFolder": "Document/Meetings"}"#));
    assert!(r.get("error").is_none(), "{r}");
    let (_, new_dir) = meeting_with(
        &app,
        T0 + 60_000,
        vec![tokens("gpt-4o-mini", at, 2_000_000)],
    );
    assert_ne!(old_dir, new_dir);
    // Copied from another device into the current folder: counted (records carry no device id).
    let mut copy = read_sidecar(&old_dir, &old);
    copy.id = "meeting-20261007-220000".into();
    copy.segments.clear();
    meeting::sidecar::write(&new_dir, &copy).unwrap();
    // In a folder this device doesn't list: not counted.
    let elsewhere = e.folder("Document/Other");
    copy.id = "meeting-20261007-220500".into();
    meeting::sidecar::write(&elsewhere, &copy).unwrap();
    let r = parse(&app.spend_json("2026-10", "2026-10"));
    let oct = &r["months"][0];
    assert_eq!(
        (oct["usd"].clone(), oct["meetings"].clone()),
        (json!(0.6), json!(3))
    );
    assert_eq!(
        r["notes"],
        json!([
            "Counted from the meetings in this device's save folders",
            "Failed requests aren't counted"
        ])
    );
}

/// F16-FR8: a usage entry this build can't read (a hand edit, a newer shape) never makes the
/// meeting unreadable: it is kept as written and shown as "+".
#[test]
fn an_unreadable_usage_entry_degrades_to_plus() {
    let e = Env::new();
    let app = e.app();
    let at = "2026-10-07T22:00:00-07:00";
    let (a, dir) = meeting_with(&app, T0, vec![tokens("gpt-4o-mini", at, 1_000_000)]);
    let path = dir.join(format!("{a}.json"));
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let bad = json!({"id": "u-1", "usd": "lots"});
    v["usage"].as_array_mut().unwrap().push(bad.clone());
    std::fs::write(&path, v.to_string()).unwrap();
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["source"], "sidecar", "still readable");
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $0.15+");
    assert_eq!(
        parse(&app.spend_json("2026-10", "2026-10"))["months"][0]["usd"],
        0.15
    );
    // An edit and a re-price keep the entry as written.
    app.rename_json(&a, "Kept");
    parse(&app.reprice_json(false));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["usage"][1], bad);
    assert_eq!(v["title"], "Kept");
    // Not even a list: kept, and the meeting still lists.
    let mut v = v;
    v["usage"] = json!(5);
    std::fs::write(&path, v.to_string()).unwrap();
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $0.00+");
    let v = parse(&app.get_meeting_json(&a));
    assert_eq!(v["meeting"]["usage"], json!([5]));
    v["meeting"]["usage"][0].as_u64().unwrap();
    // `null` is none.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["usage"] = Value::Null;
    std::fs::write(&path, v.to_string()).unwrap();
    assert_eq!(
        row(&parse(&app.list_recent_json(5)), &a)["spend"]["label"],
        "—"
    );
}
