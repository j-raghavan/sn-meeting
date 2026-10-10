//! F16-FR2 / INV14 / F16-AC1 against a mock server: every reply a job gets leaves one priced usage
//! record, in the same sidecar write as its result (or its failure); failed requests leave none;
//! retries and "again" runs add records; no record holds a key.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::providers::Endpoints;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const SUMMARY: &str = r#"{"title":"Sync","summary":"S","decisions":[],"todos":[],"sections":[]}"#;

struct Fx {
    _e: Env,
    app: Arc<App>,
    id: String,
    dir: PathBuf,
}

/// A recorded two-segment meeting (30 s and 20 s) with an OpenAI key that transcribes with
/// whisper-1.
fn fixture(uri: &str) -> Fx {
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    let body =
        format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\ntranscribe_model=whisper-1\n");
    std::fs::write(key_dir.join("copilot-key-openai.txt"), body).unwrap();
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
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    let dir = first.parent().unwrap().to_path_buf();
    std::fs::write(&first, b"seg1").unwrap();
    app.close_segment_json(&id, 0, 30_000);
    let second = dir.join(format!("{id}-02.m4a"));
    std::fs::write(&second, b"seg2").unwrap();
    app.add_segment_json(&id, second.to_str().unwrap(), 30_000);
    app.close_segment_json(&id, 1, 20_000);
    app.finish_json(&id, 50_000, "");
    Fx {
        _e: e,
        app: Arc::new(app),
        id,
        dir,
    }
}

fn whisper(n: u32, seconds: f64) -> Mock {
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(body_string_contains(format!("-0{n}.m4a")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": format!("text {n}"), "usage": {"type": "duration", "seconds": seconds}
        })))
}

fn summary(text: &str, status: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "status": status,
            "model": "gpt-4o-mini-2024-07-18",
            "incomplete_details": if status == "incomplete" {
                json!({"reason": "max_output_tokens"})
            } else {
                Value::Null
            },
            "output": [{"type": "message", "id": "m", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": text, "annotations": []}]}],
            "usage": {"input_tokens": 1000, "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": 500, "total_tokens": 1500}
        })))
}

async fn run(fx: &Fx, job: Job) -> Value {
    let (app, id) = (fx.app.clone(), fx.id.clone());
    let out = tokio::task::spawn_blocking(move || app.run_job_json(&id, job))
        .await
        .unwrap();
    parse(&out)
}

fn sidecar(fx: &Fx) -> Value {
    let raw = std::fs::read_to_string(fx.dir.join(format!("{}.json", fx.id))).unwrap();
    assert!(!raw.contains(OA), "key leaked into the sidecar");
    parse(&raw)
}

fn usage(fx: &Fx) -> Vec<Value> {
    sidecar(fx)["usage"].as_array().cloned().unwrap_or_default()
}

/// F16-AC1: whisper-1 reports 30 s; the record is priced at the default rate.
#[tokio::test(flavor = "multi_thread")]
async fn each_segment_and_the_summary_leave_one_priced_record() {
    let s = MockServer::start().await;
    whisper(1, 30.0).mount(&s).await;
    whisper(2, 20.0).mount(&s).await;
    summary(SUMMARY, "completed").mount(&s).await;
    let fx = fixture(&s.uri());
    assert!(
        sidecar(&fx).get("usage").is_none(),
        "no list until there is a record"
    );
    let out = run(&fx, Job::Transcribe).await;
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let u = usage(&fx);
    assert_eq!(u.len(), 3, "{u:?}");
    let first = &u[0];
    assert_eq!(first["kind"], "transcribe");
    assert_eq!(first["segment"], 0);
    assert_eq!(first["model"], "whisper-1");
    assert_eq!(first["billedSeconds"], 30.0);
    assert_eq!(first["usd"], 0.003);
    assert_eq!(first["source"], "provider");
    assert_eq!(first["billing"], "minute");
    assert_eq!(first["audioMs"], 30_000);
    assert_eq!(first["rate"]["asOf"], meeting::rates::DEFAULTS_AS_OF);
    assert!(
        first["at"].as_str().unwrap().ends_with("-07:00"),
        "local time"
    );
    assert_eq!(u[1]["segment"], 1);
    assert_eq!(u[1]["usd"], 0.002);
    let sum = &u[2];
    assert_eq!(sum["kind"], "summarise");
    assert!(sum.get("segment").is_none());
    assert_eq!(sum["model"], "gpt-4o-mini-2024-07-18");
    // 1 000 × 0.15 + 500 × 0.60 per 1M.
    assert_eq!(sum["usd"], 0.00045);
    assert_ne!(u[0]["id"], u[1]["id"]);
    // "Summarise again" adds a record; nothing is replaced.
    let out = run(&fx, Job::Summarise).await;
    assert_eq!(out["meeting"]["status"], "done");
    let again = usage(&fx);
    assert_eq!(again.len(), 4);
    assert_eq!(again[..3], u[..]);
}

/// OQ16: a 503 reports no usage and leaves no record; the retry's reply does.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_request_leaves_no_record_and_its_retry_one() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&s)
        .await;
    whisper(1, 30.0).mount(&s).await;
    whisper(2, 20.0).mount(&s).await;
    let fx = fixture(&s.uri());
    // No summarise key problem: the summary fails on an unmatched route (404), recording none.
    let out = run(&fx, Job::Transcribe).await;
    assert_eq!(out["meeting"]["status"], "failed", "{out}");
    let u = usage(&fx);
    let kinds: Vec<_> = u.iter().map(|r| r["segment"].clone()).collect();
    assert_eq!(
        kinds,
        [json!(0), json!(1)],
        "one per reply, none for the 503 or the 404"
    );
}

/// A long segment whose reply is empty was billed, then retried: both are spent.
#[tokio::test(flavor = "multi_thread")]
async fn a_billed_reply_that_fails_is_recorded_in_the_failure_write() {
    let s = MockServer::start().await;
    let empty = json!({"text": "", "usage": {"type": "duration", "seconds": 45}});
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty))
        .mount(&s)
        .await;
    let fx = fixture(&s.uri());
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.segments[0].duration_ms = Some(45_000);
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    // The retry waits RETRY_DEFAULT (5 s) without a Retry-After header.
    let out = run(&fx, Job::Transcribe).await;
    let m = &out["meeting"];
    assert_eq!(m["status"], "failed");
    assert!(m["segments"][0]["text"].is_null());
    let u = usage(&fx);
    assert_eq!(u.len(), 2, "both billed attempts: {u:?}");
    assert!(u.iter().all(|r| r["usd"] == 0.0045 && r["segment"] == 0));
}

/// A cut-off summary fails summarising, but its tokens are recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_cut_off_summary_is_recorded() {
    let s = MockServer::start().await;
    whisper(1, 30.0).mount(&s).await;
    whisper(2, 20.0).mount(&s).await;
    summary("{\"title\"", "incomplete").mount(&s).await;
    let fx = fixture(&s.uri());
    let out = run(&fx, Job::Transcribe).await;
    assert_eq!(out["meeting"]["failedAt"], "summarising", "{out}");
    let u = usage(&fx);
    assert_eq!(u.len(), 3);
    assert_eq!(u[2]["kind"], "summarise");
    assert_eq!(u[2]["outputTokens"], 500);
}

/// A summary that isn't our JSON fails after a successful call: its usage is still kept.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_summary_is_recorded() {
    let s = MockServer::start().await;
    whisper(1, 30.0).mount(&s).await;
    whisper(2, 20.0).mount(&s).await;
    summary("not json", "completed").mount(&s).await;
    let fx = fixture(&s.uri());
    let out = run(&fx, Job::Transcribe).await;
    assert_eq!(out["meeting"]["failedAt"], "summarising", "{out}");
    assert_eq!(usage(&fx).len(), 3);
}

/// Usage only grows: "Transcribe again" keeps the first run's records.
#[tokio::test(flavor = "multi_thread")]
async fn transcribe_again_adds_records() {
    let s = MockServer::start().await;
    whisper(1, 30.0).mount(&s).await;
    whisper(2, 20.0).mount(&s).await;
    summary(SUMMARY, "completed").mount(&s).await;
    let fx = fixture(&s.uri());
    run(&fx, Job::Transcribe).await;
    let out = run(&fx, Job::TranscribeAgain).await;
    assert_eq!(out["meeting"]["status"], "done");
    assert_eq!(usage(&fx).len(), 6);
    // An edit after the job keeps them.
    fx.app.rename_json(&fx.id, "Renamed");
    assert_eq!(usage(&fx).len(), 6);
}
