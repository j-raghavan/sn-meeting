//! F5-FR2 / FR3 against a mock server: per-segment text written as it arrives, markers from
//! `startOffsetMs`, resume sending only `null` segments, missing audio, no speech, cancel.
mod libcommon;

use libcommon::*;
use meeting::jobs::Cancel;
use meeting::keyfile::{self, Provider};
use meeting::pipeline::{self, JobError, JobPlan};
use meeting::providers::{Endpoints, http_client};
use meeting::status::{FailedAt, Phase};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";

struct Fx {
    e: Env,
    app: meeting::app::App,
    id: String,
    dir: PathBuf,
}

/// A recorded meeting with three segments at 0, 15:00 and 30:00.4.
fn meeting() -> Fx {
    let e = Env::new();
    let app = e.app();
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    let dir = first.parent().unwrap().to_path_buf();
    std::fs::write(&first, b"seg1").unwrap();
    app.close_segment_json(&id, 0, 900_000);
    for (n, off) in [(2, 900_000), (3, 1_800_400)] {
        let p = dir.join(format!("{id}-0{n}.m4a"));
        std::fs::write(&p, format!("seg{n}")).unwrap();
        app.add_segment_json(&id, p.to_str().unwrap(), off);
        app.close_segment_json(&id, n - 1, 600_000);
    }
    app.finish_json(&id, 2_400_000, "");
    Fx { e, app, id, dir }
}

fn plan(uri: &str) -> JobPlan {
    let key = keyfile::parse(
        &format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\n"),
        Provider::OpenAi,
    )
    .unwrap();
    JobPlan {
        http: http_client(Duration::from_secs(5), false).unwrap(),
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.into(),
            anthropic: uri.into(),
        },
        transcribe: Some((key, "gpt-transcribe".into())),
        default_template: "general".into(),
    }
}

async fn segment(s: &MockServer, n: u32, delay: Duration, text: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(body_string_contains(format!("-0{n}.m4a")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
                .set_body_json(json!({"text": text})),
        )
        .mount(s)
        .await;
}

async fn run(fx: &Fx, uri: &str, cancel: &Cancel) -> Result<Phase, JobError> {
    let lock = Mutex::new(());
    pipeline::transcribe(&fx.app.library(), &lock, &fx.id, &plan(uri), cancel).await
}

fn sidecar(fx: &Fx) -> Value {
    json!(read_sidecar(&fx.dir, &fx.id))
}

/// Which segment files the server was sent, in order.
async fn sent(s: &MockServer) -> Vec<u32> {
    let mut out = Vec::new();
    for r in s.received_requests().await.unwrap() {
        let b = String::from_utf8_lossy(&r.body);
        out.extend((1..=3).filter(|n| b.contains(&format!("-0{n}.m4a"))));
    }
    out
}

#[tokio::test]
async fn segments_are_stored_in_order_with_markers_and_stamps() {
    // F5-AC1 (later segments answer first is irrelevant: one request at a time, in order),
    // F5-AC10, EC6 for an empty reply.
    let s = MockServer::start().await;
    segment(&s, 1, Duration::from_millis(150), "first").await;
    segment(&s, 2, Duration::ZERO, " ").await;
    segment(&s, 3, Duration::ZERO, "third").await;
    let fx = meeting();
    assert_eq!(
        run(&fx, &s.uri(), &Cancel::default()).await.unwrap(),
        Phase::Transcribed
    );
    assert_eq!(sent(&s).await, [1, 2, 3]);
    let sc = read_sidecar(&fx.dir, &fx.id);
    let texts: Vec<_> = sc
        .segments
        .iter()
        .map(|s| s.text.clone().unwrap())
        .collect();
    assert_eq!(texts, ["first", "(no speech detected)", "third"]);
    assert_eq!(
        pipeline::joined(&sc),
        "[0:00:00] first\n\n[0:15:00] (no speech detected)\n\n[0:30:00] third"
    );
    let v = sidecar(&fx);
    assert_eq!(v["status"], "transcribed");
    assert_eq!(v["templateId"], "general");
    assert_eq!(
        v["providers"]["transcribe"],
        json!({"provider": "openai", "model": "gpt-transcribe"})
    );
    assert!(v["error"].is_null());
    assert!(!v.to_string().contains(OA));
}

#[tokio::test]
async fn a_failed_segment_is_the_only_one_sent_on_retry() {
    // F5-AC3 / F5-FR3, with the failure user-worded (F5-AC2).
    let s = MockServer::start().await;
    segment(&s, 1, Duration::ZERO, "one").await;
    Mock::given(method("POST"))
        .and(body_string_contains("-02.m4a"))
        .respond_with(ResponseTemplate::new(401).set_body_string(format!("bad {OA}")))
        .mount(&s)
        .await;
    let fx = meeting();
    let p = run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(p, Phase::Failed(FailedAt::Transcribing));
    let v = sidecar(&fx);
    assert_eq!(v["error"], "OpenAI rejected the key (401)");
    assert_eq!(v["segments"][0]["text"], "one");
    assert!(v["segments"][1]["text"].is_null() && v["segments"][2]["text"].is_null());
    assert_eq!(sent(&s).await, [1, 2]);

    s.reset().await;
    segment(&s, 2, Duration::ZERO, "two").await;
    segment(&s, 3, Duration::ZERO, "three").await;
    assert_eq!(
        run(&fx, &s.uri(), &Cancel::default()).await.unwrap(),
        Phase::Transcribed
    );
    assert_eq!(sent(&s).await, [2, 3], "segment 1 is not re-sent");
    assert!(sidecar(&fx)["error"].is_null());
}

#[tokio::test]
async fn a_413_fails_without_a_retry() {
    // F5-AC8.
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(413))
        .mount(&s)
        .await;
    let fx = meeting();
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(sidecar(&fx)["error"], "Segment too large for OpenAI");
    assert_eq!(s.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_missing_segment_fails_the_meeting() {
    // EC7: deleted on the desktop. Segment 1 is sent first; the job stops at segment 2.
    let s = MockServer::start().await;
    segment(&s, 1, Duration::ZERO, "one").await;
    let fx = meeting();
    std::fs::remove_file(fx.dir.join(format!("{}-02.m4a", fx.id))).unwrap();
    let p = run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(p, Phase::Failed(FailedAt::Transcribing));
    let v = sidecar(&fx);
    assert_eq!(v["error"], "Segment 2 is missing");
    assert_eq!(v["segments"][0]["text"], "one");
    // A segment recovery flagged missing is not sent even if a file reappears.
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.segments[1].flag = Some(meeting::sidecar::SegmentFlag::Missing);
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    std::fs::write(fx.dir.join(format!("{}-02.m4a", fx.id)), b"x").unwrap();
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(sidecar(&fx)["error"], "Segment 2 is missing");
}

#[tokio::test]
async fn an_oversized_or_unreadable_segment_is_not_sent() {
    let s = MockServer::start().await;
    let fx = meeting();
    let f = std::fs::File::create(fx.dir.join(format!("{}-01.m4a", fx.id))).unwrap();
    f.set_len(pipeline::MAX_AUDIO_BYTES + 1).unwrap();
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(sidecar(&fx)["error"], "Segment too large for OpenAI");
    std::fs::remove_file(fx.dir.join(format!("{}-01.m4a", fx.id))).unwrap();
    std::fs::create_dir(fx.dir.join(format!("{}-01.m4a", fx.id))).unwrap();
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    let err = sidecar(&fx)["error"].as_str().unwrap().to_owned();
    assert!(err.starts_with("Segment 1 can't be read"), "{err}");
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancel_keeps_finished_segments_and_stops_the_request() {
    // F5-AC13.
    let s = MockServer::start().await;
    segment(&s, 1, Duration::ZERO, "one").await;
    segment(&s, 2, Duration::from_secs(20), "late").await;
    let fx = Arc::new(meeting());
    let c = Cancel::default();
    let (c2, fx2) = (c.clone(), fx.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if sidecar(&fx2)["segments"][0]["text"] == "one" {
                break;
            }
        }
        c2.cancel();
    });
    let t0 = Instant::now();
    let p = run(&fx, &s.uri(), &c).await.unwrap();
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    assert_eq!(p, Phase::Failed(FailedAt::Transcribing));
    let v = sidecar(&fx);
    assert_eq!(v["error"], "Cancelled");
    assert_eq!(v["segments"][0]["text"], "one");
    assert!(
        v["segments"][1]["text"].is_null(),
        "the in-flight segment stays null"
    );
    // Cancelled before the next segment starts: nothing more is sent.
    let c = Cancel::default();
    c.cancel();
    s.reset().await;
    assert_eq!(
        run(&fx, &s.uri(), &c).await.unwrap(),
        Phase::Failed(FailedAt::Transcribing)
    );
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_meeting_that_cant_transcribe_now_starts_nothing() {
    let s = MockServer::start().await;
    segment(&s, 1, Duration::ZERO, "a").await;
    segment(&s, 2, Duration::ZERO, "b").await;
    segment(&s, 3, Duration::ZERO, "c").await;
    let fx = meeting();
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    // Transcribed: Transcribe is not offered.
    let e = run(&fx, &s.uri(), &Cancel::default()).await.unwrap_err();
    assert!(matches!(e, JobError::NotStarted(_)), "{e:?}");
    // A summarise-only plan can't transcribe.
    let mut p = plan(&s.uri());
    p.transcribe = None;
    let lock = Mutex::new(());
    let e = pipeline::transcribe(&fx.app.library(), &lock, &fx.id, &p, &Cancel::default()).await;
    assert!(matches!(e, Err(JobError::NotStarted(_))));
    // The template chosen for the meeting is kept.
    let fx = meeting();
    fx.app.set_template_json(&fx.id, "standup");
    run(&fx, &s.uri(), &Cancel::default()).await.unwrap();
    assert_eq!(sidecar(&fx)["templateId"], "standup");
    let _ = &fx.e;
}
