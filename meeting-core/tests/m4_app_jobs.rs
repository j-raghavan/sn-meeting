//! F5-FR10 and the job runner: providers snapshotted at job start, a running job counted as
//! live (recover leaves it, delete refuses it, a second job is busy), cancel from any thread.
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
const GE: &str = "AIza-SECRET-gemini-0003";

struct Fx {
    _e: Env,
    app: Arc<App>,
    id: String,
    dir: PathBuf,
}

fn app_for(e: &Env, uri: &str, keys: &[&str]) -> App {
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    for p in keys {
        let (k, m) = match *p {
            "openai" => (OA, "gpt-4o-mini"),
            _ => (GE, "gemini-3.8-flash"),
        };
        let body = format!("provider={p}\nkey={k}\nmodel={m}\n");
        std::fs::write(key_dir.join(format!("copilot-key-{p}.txt")), body).unwrap();
    }
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
    App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)))
}

/// A recorded two-segment meeting.
fn fixture(uri: &str, keys: &[&str]) -> Fx {
    let e = Env::new();
    let app = app_for(&e, uri, keys);
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    let dir = first.parent().unwrap().to_path_buf();
    std::fs::write(&first, b"seg1").unwrap();
    let second = dir.join(format!("{id}-02.m4a"));
    std::fs::write(&second, b"seg2").unwrap();
    app.add_segment_json(&id, second.to_str().unwrap(), 900_000);
    app.finish_json(&id, 1_800_000, "");
    Fx {
        _e: e,
        app: Arc::new(app),
        id,
        dir,
    }
}

async fn openai_segment(s: &MockServer, n: u32, delay: Duration) {
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(body_string_contains(format!("-0{n}.m4a")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
                .set_body_json(json!({"text": format!("text {n}")})),
        )
        .mount(s)
        .await;
}

fn meeting(fx: &Fx) -> Value {
    json!(read_sidecar(&fx.dir, &fx.id))
}

fn spawn(fx: &Fx) -> std::thread::JoinHandle<String> {
    let (app, id) = (fx.app.clone(), fx.id.clone());
    std::thread::spawn(move || app.run_job_json(&id, Job::Transcribe))
}

fn wait_for(fx: &Fx, f: impl Fn(&Value) -> bool) {
    for _ in 0..250 {
        if f(&meeting(fx)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting: {}", meeting(fx));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_changed_mid_job_do_not_reach_it() {
    let s = MockServer::start().await;
    openai_segment(&s, 1, Duration::ZERO).await;
    openai_segment(&s, 2, Duration::from_millis(600)).await;
    openai_summary(&s).await;
    let fx = fixture(&s.uri(), &["openai", "gemini"]);
    let h = spawn(&fx);
    wait_for(&fx, |m| m["segments"][0]["text"] == "text 1");
    let patch = r#"{"transcribe":{"provider":"gemini","model":"other"},
        "summarise":{"provider":"gemini"},"defaultTemplate":"standup"}"#;
    let r = parse(&fx.app.set_settings_json(patch));
    assert!(r.get("error").is_none(), "{r}");
    let out = parse(
        &tokio::task::spawn_blocking(move || h.join().unwrap())
            .await
            .unwrap(),
    );
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let m = meeting(&fx);
    assert_eq!(
        m["providers"]["transcribe"],
        json!({"provider": "openai", "model": "gpt-transcribe"})
    );
    assert_eq!(
        m["providers"]["summarise"],
        json!({"provider": "openai", "model": "gpt-4o-mini"})
    );
    assert_eq!(m["templateId"], "general");
    let paths: Vec<String> = s
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    let t = "/v1/audio/transcriptions";
    // The job lists the account's models first (D2: unmocked here, so the default runs).
    assert_eq!(paths, ["/v1/models", t, t, "/v1/responses"]);
}

const SUMMARY: &str =
    r#"{"title":"Deck review","summary":"S","decisions":[],"todos":[],"sections":[]}"#;

async fn openai_summary(s: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "status": "completed",
            "model": "gpt-4o-mini",
            "output": [{"type": "message", "id": "m", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": SUMMARY, "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
        })))
        .mount(s)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_job_is_live_busy_and_cancellable() {
    let s = MockServer::start().await;
    openai_segment(&s, 1, Duration::ZERO).await;
    openai_segment(&s, 2, Duration::from_secs(20)).await;
    let fx = fixture(&s.uri(), &["openai"]);
    let h = spawn(&fx);
    wait_for(&fx, |m| m["segments"][0]["text"] == "text 1");
    assert!(fx.app.liveness().jobs.contains(&fx.id));
    // Recovery while the job runs must not fail it (F3-FR5).
    let r = parse(&fx.app.recover_json(false));
    assert_eq!(r["recovered"], json!([]));
    assert_eq!(meeting(&fx)["status"], "transcribing");
    assert_eq!(
        parse(&fx.app.run_job_json(&fx.id, Job::Transcribe)),
        json!({"error": "meeting is busy"})
    );
    assert_eq!(
        parse(&fx.app.delete_json(&fx.id)),
        json!({"error": "meeting is being transcribed"})
    );
    let t0 = std::time::Instant::now();
    assert_eq!(
        parse(&fx.app.cancel_json(&fx.id)),
        json!({"cancelled": true})
    );
    let out = parse(
        &tokio::task::spawn_blocking(move || h.join().unwrap())
            .await
            .unwrap(),
    );
    assert!(t0.elapsed() < Duration::from_secs(3));
    let m = &out["meeting"];
    assert_eq!(
        (m["status"].as_str(), m["error"].as_str()),
        (Some("failed"), Some("Cancelled"))
    );
    assert_eq!(m["segments"][0]["text"], "text 1");
    assert!(m["segments"][1]["text"].is_null());
    assert_eq!(
        parse(&fx.app.cancel_json(&fx.id)),
        json!({"cancelled": false})
    );
    assert!(
        fx.app.liveness().jobs.is_empty(),
        "the guard freed the meeting"
    );
    assert!(!out.to_string().contains(OA));
}

#[test]
fn nothing_starts_without_a_key_or_in_the_wrong_status() {
    let fx = fixture("http://127.0.0.1:9", &[]);
    assert_eq!(
        parse(&fx.app.run_job_json(&fx.id, Job::Transcribe)),
        json!({"error": "Import copilot-key-openai.txt in Settings, or add it to MyStyle/SnCopilot"})
    );
    assert_eq!(meeting(&fx)["status"], "recorded", "untouched");
    let fx = fixture("http://127.0.0.1:9", &["openai"]);
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.status = meeting::status::Status::Done;
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    assert_eq!(
        parse(&fx.app.run_job_json(&fx.id, Job::Transcribe)),
        json!({"error": "Transcribe isn't available while done"})
    );
    assert_eq!(
        parse(
            &fx.app
                .run_job_json("meeting-20990101-000000", Job::Transcribe)
        ),
        json!({"error": "meeting not found"})
    );
    assert!(fx.app.liveness().jobs.is_empty());
}

#[test]
fn a_dead_network_fails_the_meeting_and_transcribe_is_offered_again() {
    // F5-AC9: within the budget the meeting is failed(transcribing) with "No connection".
    let fx = fixture("http://127.0.0.1:9", &["openai"]);
    let out = parse(&fx.app.run_job_json(&fx.id, Job::Transcribe));
    let m = &out["meeting"];
    assert_eq!(m["status"], "failed");
    assert_eq!(m["failedAt"], "transcribing");
    assert_eq!(m["error"], "No connection");
}
