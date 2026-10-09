//! F16-FR5 / F16-AC5 / F16-AC6 through `App`: a meeting's cost on its Home row and in
//! nativeGetMeeting, and nativeSpend's months, from the usage records in the sidecars.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::keyfile::Provider;
use meeting::providers::Measured;
use meeting::rates::RateTable;
use meeting::spend::{Kind, UsageRecord};
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
    sc.usage = records;
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
    assert_eq!(before.usage[0].usd, Some(0.3));
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
    assert_eq!(after.usage[0].usd, Some(1.0));
    assert_eq!(after.usage[1].usd, Some(0.11));
    assert_eq!(after.usage[0].input_tokens, before.usage[0].input_tokens);
    assert_eq!(after.usage[0].id, before.usage[0].id);
    assert_eq!(
        after.usage[0].rate.as_ref().unwrap().as_of,
        app.today().format("%Y-%m-%d").to_string()
    );
    let list = parse(&app.list_recent_json(5));
    assert_eq!(row(&list, &a)["spend"]["label"], "≈ $1.11", "a full total");
    assert_eq!(row(&list, &b)["spend"]["label"], "≈ $0.50");
    // Nothing left to change.
    assert_eq!(parse(&app.reprice_json(false))["meetings"], 0);
}
