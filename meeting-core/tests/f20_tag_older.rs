//! F20-FR10 / OQ31: "Tag older meetings…" runs only on request. A dry run counts the untagged
//! meetings and prices the calls first; the run makes one tags-only call per meeting on its
//! stored summary and transcript, records each call's usage, and leaves tagged meetings (and
//! ones whose tags the user removed) alone. Mock provider only.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::App;
use meeting::providers::Endpoints;
use meeting::sidecar::{Tag, TagSource};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const AN: &str = "sk-ant-SECRET-anthropic-0004";
const HOUR: i64 = 3_600_000;

fn app_at(e: &Env, uri: &str) -> App {
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    std::fs::write(
        key_dir.join("copilot-key-anthropic.txt"),
        format!("provider=anthropic\nkey={AN}\nmodel=claude-x\n"),
    )
    .unwrap();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.into(),
            anthropic: uri.into(),
        },
        timeout: Duration::from_secs(5),
        https_only: false,
    };
    let app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    app.set_settings_json(r#"{"summarise":{"provider":"anthropic"}}"#);
    app
}

fn meeting(
    app: &App,
    at: i64,
    summary: Option<&str>,
    f: impl FnOnce(&mut meeting::sidecar::Sidecar),
) -> (String, PathBuf) {
    let b = begin(app, at);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 60_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut sc = read_sidecar(&dir, &id);
    sc.summary = summary.map(Into::into);
    f(&mut sc);
    meeting::sidecar::write(&dir, &sc).unwrap();
    (id, dir)
}

async fn reply(s: &MockServer, text: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m", "type": "message", "role": "assistant", "model": "claude-x",
            "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 400, "output_tokens": 30}
        })))
        .mount(s)
        .await;
}

#[tokio::test]
async fn older_meetings_are_tagged_on_request_with_the_cost_shown_first() {
    let s = MockServer::start().await;
    reply(&s, r#"{"tags":["GPU","Local AI"]}"#).await;
    let e = Env::new();
    let app = Arc::new(app_at(&e, &s.uri()));
    let (old, dir) = meeting(&app, T0, Some("We compared graphics cards."), |_| {});
    let (done, _) = meeting(&app, T0 + HOUR, Some("Budget"), |sc| {
        sc.tags = vec![Tag::new("budget", TagSource::Ai)];
    });
    let (removed, _) = meeting(&app, T0 + 2 * HOUR, Some("Hiring"), |sc| {
        sc.tags_user_removed = vec!["hiring".into()];
    });
    let (empty, _) = meeting(&app, T0 + 3 * HOUR, None, |_| {});

    let a = app.clone();
    let dry: Value = parse(
        &tokio::task::spawn_blocking(move || a.tag_older_json(true))
            .await
            .unwrap(),
    );
    assert_eq!(dry["meetings"], 1, "{dry}");
    assert_eq!(dry["provider"], "anthropic");
    assert_eq!(dry["model"], "claude-x");
    assert_eq!(
        dry["estUsd"],
        Value::Null,
        "an unpriced model has no estimate"
    );
    assert!(
        s.received_requests().await.unwrap().is_empty(),
        "a dry run calls nothing"
    );
    let r = app.set_rates_json(
        &json!([{"provider": "anthropic", "model": "claude-x", "unit": "tokens",
                 "input": 1.0, "output": 5.0}])
        .to_string(),
    );
    assert!(!r.contains("\"error\""), "{r}");
    let a = app.clone();
    let dry: Value = parse(
        &tokio::task::spawn_blocking(move || a.tag_older_json(true))
            .await
            .unwrap(),
    );
    assert!(dry["estUsd"].as_f64().unwrap() > 0.0, "{dry}");

    let a = app.clone();
    let run: Value = parse(
        &tokio::task::spawn_blocking(move || a.tag_older_json(false))
            .await
            .unwrap(),
    );
    assert_eq!(run, json!({"tagged": 1, "failed": 0, "error": null}));
    let reqs = s.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1, "one tags-only call per untagged meeting");
    let wire = String::from_utf8_lossy(&reqs[0].body).to_string();
    assert!(
        wire.contains("We compared graphics cards."),
        "the stored summary goes"
    );
    assert!(wire.contains("Existing tags:"));
    assert!(wire.contains("budget"), "the vocabulary goes: {wire}");
    let sc = read_sidecar(&dir, &old);
    let names: Vec<&str> = sc.tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["gpu", "local-ai"]);
    assert!(sc.tags.iter().all(|t| t.source == TagSource::Ai));
    assert_eq!(sc.usage.len(), 1, "the call's usage is recorded");
    assert_eq!(read_sidecar(&dir, &done).tags.len(), 1);
    assert!(read_sidecar(&dir, &removed).tags.is_empty());
    assert!(read_sidecar(&dir, &empty).tags.is_empty());
    // Nothing left to tag.
    let a = app.clone();
    let again: Value = parse(
        &tokio::task::spawn_blocking(move || a.tag_older_json(true))
            .await
            .unwrap(),
    );
    assert_eq!(again["meetings"], 0);
}

#[tokio::test]
async fn a_failed_call_is_counted_and_its_meeting_left_untagged() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&s)
        .await;
    let e = Env::new();
    let app = Arc::new(app_at(&e, &s.uri()));
    let (id, dir) = meeting(&app, T0, Some("Something"), |_| {});
    let a = app.clone();
    let run: Value = parse(
        &tokio::task::spawn_blocking(move || a.tag_older_json(false))
            .await
            .unwrap(),
    );
    assert_eq!(run["tagged"], 0);
    assert_eq!(run["failed"], 1);
    assert!(run["error"].is_string(), "{run}");
    assert!(!run.to_string().contains(AN), "never the key");
    assert!(read_sidecar(&dir, &id).tags.is_empty());
}

#[test]
fn without_a_key_it_says_so() {
    let e = Env::new();
    let app = e.app();
    assert!(parse(&app.tag_older_json(true)).get("error").is_some());
}
