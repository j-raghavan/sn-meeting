//! F4-AC2 / INV2: full transcribe and summarise runs against the mock server for every
//! provider, on success and on 401, 500 and timeout replies that echo the key; no reply and no
//! file the core wrote (sidecar, transcript, settings) may contain any fixture key.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::jobs::Budgets;
use meeting::providers::Endpoints;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{body_string_contains, method, path, path_regex};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const GE: &str = "AIza-SECRET-gemini-0003";
const SUMMARY: &str = r#"{"title":"T","summary":"S","decisions":[],"todos":[{"text":"x","owner":null,"due":null}],"sections":[]}"#;

#[derive(Clone, Copy, Debug)]
enum Reply {
    Ok,
    Unauthorised,
    ServerError,
    Slow,
}

fn key_of(p: &str) -> &'static str {
    match p {
        "openai" => OA,
        "gemini" => GE,
        _ => AN,
    }
}

fn transcribe_route(p: &str) -> MockBuilder {
    match p {
        "openai" => Mock::given(method("POST")).and(path("/v1/audio/transcriptions")),
        _ => Mock::given(method("POST"))
            .and(path_regex(":generateContent$"))
            .and(body_string_contains("inlineData")),
    }
}

fn summarise_route(p: &str) -> MockBuilder {
    match p {
        "openai" => Mock::given(method("POST")).and(path("/v1/responses")),
        "gemini" => Mock::given(method("POST"))
            .and(path_regex(":generateContent$"))
            .and(body_string_contains("responseJsonSchema")),
        _ => Mock::given(method("POST")).and(path("/v1/messages")),
    }
}

fn success(p: &str, text: &str) -> ResponseTemplate {
    let body = match p {
        "openai" if text != SUMMARY => json!({"text": text}),
        "openai" => json!({
            "id": "r", "object": "response", "created_at": 1, "status": "completed", "model": "m",
            "output": [{"type": "message", "id": "m", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": text, "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}),
        "gemini" => json!({"candidates": [{"content": {"parts": [{"text": text}], "role": "model"},
            "finishReason": "STOP"}]}),
        _ => json!({"id": "m", "type": "message", "role": "assistant", "model": "m",
            "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1}}),
    };
    ResponseTemplate::new(200).set_body_json(body)
}

fn reply(p: &str, r: Reply, text: &str) -> ResponseTemplate {
    let k = key_of(p);
    let echo = format!(
        r#"{{"error":{{"message":"key {k} Authorization: Bearer {k} x-goog-api-key: {k}"}}}}"#
    );
    match r {
        Reply::Ok => success(p, text),
        Reply::Unauthorised => ResponseTemplate::new(401).set_body_string(echo),
        Reply::ServerError => ResponseTemplate::new(500)
            .insert_header("retry-after", "0")
            .set_body_string(echo),
        Reply::Slow => success(p, text).set_delay(Duration::from_secs(3)),
    }
}

/// One full run: transcribe with `tp`, summarise with `sp`, the stage under test answering
/// `r`. Returns every reply the API gave.
fn run(tp: &str, sp: &str, transcribe_reply: Reply, summarise_reply: Reply) -> (Env, Vec<String>) {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let keys = e.data.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    for (p, m) in [
        ("openai", "gpt-4o-mini"),
        ("gemini", "gemini-3.8-flash"),
        ("claude", "claude-x"),
    ] {
        let id = if p == "claude" { "anthropic" } else { p };
        let body = format!("provider={id}\nkey={}\nmodel={m}\n", key_of(id));
        std::fs::write(keys.join(format!("copilot-key-{p}.txt")), body).unwrap();
    }
    let uri = s.uri();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir: keys,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.clone(),
            anthropic: uri,
        },
        timeout: Duration::from_secs(5),
        https_only: false,
    };
    let mut app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    app.budgets = Budgets {
        transcribe_base: Duration::from_millis(300),
        summarise: Duration::from_millis(300),
    };
    let app = Arc::new(app);
    let mut replies = vec![app.set_settings_json(&format!(
        r#"{{"transcribe":{{"provider":"{tp}"}},"summarise":{{"provider":"{sp}"}}}}"#
    ))];
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    std::fs::write(&first, b"seg1").unwrap();
    replies.push(app.close_segment_json(&id, 0, 400));
    replies.push(app.finish_json(&id, 400, ""));
    rt.block_on(
        transcribe_route(tp)
            .respond_with(reply(tp, transcribe_reply, "hello"))
            .mount(&s),
    );
    rt.block_on(
        summarise_route(sp)
            .respond_with(reply(sp, summarise_reply, SUMMARY))
            .mount(&s),
    );
    replies.push(app.run_job_json(&id, Job::Transcribe));
    replies.push(app.run_job_json(&id, Job::Summarise));
    replies.push(app.get_meeting_json(&id));
    replies.push(app.list_recent_json(5));
    replies.push(app.open_todos_json());
    replies.push(app.get_settings_json());
    (e, replies)
}

fn files(d: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "keys") {
                files(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

fn assert_clean(e: &Env, replies: &[String], label: &str) {
    let mut all = Vec::new();
    files(e.root.path(), &mut all);
    files(e.data.path(), &mut all);
    assert!(
        all.iter()
            .any(|f| f.extension().is_some_and(|x| x == "json")),
        "{label}: no sidecar"
    );
    let texts = all
        .iter()
        .map(|f| {
            (
                f.display().to_string(),
                String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned(),
            )
        })
        .chain(replies.iter().map(|r| ("reply".to_owned(), r.clone())));
    for (name, text) in texts {
        for k in [OA, AN, GE] {
            assert!(!text.contains(k), "{label}: key in {name}: {text}");
        }
    }
}

#[test]
fn no_key_reaches_any_file_or_reply() {
    let cases = [
        Reply::Ok,
        Reply::Unauthorised,
        Reply::ServerError,
        Reply::Slow,
    ];
    for tp in ["openai", "gemini"] {
        for r in cases {
            let (e, replies) = run(tp, "anthropic", r, Reply::Ok);
            assert_clean(&e, &replies, &format!("transcribe {tp} {r:?}"));
            let m: serde_json::Value = serde_json::from_str(&replies[5]).unwrap();
            let status = m["meeting"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            match r {
                Reply::Ok => assert_eq!(status, "done", "{tp}"),
                _ => assert_eq!(status, "failed", "{tp} {r:?}"),
            }
        }
    }
    for sp in ["openai", "gemini", "anthropic"] {
        for r in cases {
            let (e, replies) = run("openai", sp, Reply::Ok, r);
            assert_clean(&e, &replies, &format!("summarise {sp} {r:?}"));
            let m: serde_json::Value = serde_json::from_str(&replies[5]).unwrap();
            let want = match r {
                Reply::Ok => "done",
                _ => "failed",
            };
            assert_eq!(m["meeting"]["status"], want, "{sp} {r:?}: {m}");
            if let Reply::Slow = r {
                assert_eq!(m["meeting"]["error"], "Timed out");
            }
        }
    }
}
