//! F20-FR1: the summarise call returns the meeting's tags, with the device's existing tags in
//! the request so their names are reused. No extra call: the summary's usage record is the
//! only one. "Summarise again" re-tags but keeps the user's tags and never brings back one the
//! user removed (F20-AC1, AC2, AC3; mock provider, no real calls).
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::providers::Endpoints;
use meeting::sidecar::{Tag, TagSource};
use meeting::status::Status;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const AN: &str = "sk-ant-SECRET-anthropic-0003";
const SUMMARY: &str = r##"{"title":"Graphics cards","summary":"S","decisions":[],"todos":[],"sections":[],
  "tags":["GPU","Local AI","budget","gpu","#VRAM"]}"##;

struct Fx {
    _e: Env,
    s: MockServer,
    app: Arc<App>,
    id: String,
    dir: PathBuf,
}

/// A transcribed meeting about graphics cards, and an older one already tagged `gpu`.
async fn fixture() -> Fx {
    let s = MockServer::start().await;
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    std::fs::write(
        key_dir.join("copilot-key-anthropic.txt"),
        format!("provider=anthropic\nkey={AN}\nmodel=claude-x\n"),
    )
    .unwrap();
    let uri = s.uri();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.clone(),
            anthropic: uri,
        },
        timeout: Duration::from_secs(5),
        https_only: false,
    };
    let app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    app.set_settings_json(r#"{"summarise":{"provider":"anthropic"}}"#);
    let old = begin(&app, T0 - 86_400_000);
    let old_id = old["id"].as_str().unwrap().to_owned();
    app.finish_json(&old_id, 60_000, "");
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 60_000, "");
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mut o = read_sidecar(&dir, &old_id);
    o.tags = vec![Tag::new("gpu", TagSource::Ai)];
    meeting::sidecar::write(&dir, &o).unwrap();
    let mut sc = read_sidecar(&dir, &id);
    sc.status = Status::Transcribed;
    sc.segments[0].duration_ms = Some(60_000);
    sc.segments[0].text = Some("We compared graphics cards and their memory.".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m", "type": "message", "role": "assistant", "model": "claude-x",
            "content": [{"type": "text", "text": SUMMARY}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 900, "output_tokens": 1300}
        })))
        .mount(&s)
        .await;
    Fx {
        _e: e,
        s,
        app: Arc::new(app),
        id,
        dir,
    }
}

async fn summarise(f: &Fx) -> (Value, Value) {
    let (app, id) = (f.app.clone(), f.id.clone());
    let r = tokio::task::spawn_blocking(move || app.run_job_json(&id, Job::Summarise))
        .await
        .unwrap();
    let reqs = f.s.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs.last().unwrap().body).unwrap();
    (serde_json::from_str(&r).unwrap(), body)
}

fn tags(f: &Fx) -> Vec<(String, TagSource)> {
    read_sidecar(&f.dir, &f.id)
        .tags
        .into_iter()
        .map(|t| (t.name, t.source))
        .collect()
}

/// F20-AC1 / AC2.
#[tokio::test]
async fn the_stop_call_tags_the_meeting_reusing_existing_tags() {
    let f = fixture().await;
    let (r, body) = summarise(&f).await;
    assert_eq!(r["meeting"]["status"], "done", "{r}");
    let wire = body.to_string();
    assert!(
        wire.contains("Existing tags:\\n<tags>\\ngpu\\n</tags>"),
        "the vocabulary goes with the call: {wire}"
    );
    assert!(wire.contains("Reuse a tag from the existing tags when one fits"));
    assert!(wire.contains("\"tags\""), "the schema asks for tags");
    let got = tags(&f);
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["gpu", "local-ai", "budget", "vram"]);
    assert!((3..=7).contains(&got.len()));
    assert!(got.iter().all(|(_, s)| *s == TagSource::Ai));
    let sc = read_sidecar(&f.dir, &f.id);
    assert_eq!(sc.usage.len(), 1, "no extra call: one usage record");
    assert_eq!(f.s.received_requests().await.unwrap().len(), 1);
}

/// F20-AC3: a tag the user removed stays removed; the user's own tag stays.
#[tokio::test]
async fn summarise_again_keeps_the_users_choices() {
    let f = fixture().await;
    summarise(&f).await;
    let mut sc = read_sidecar(&f.dir, &f.id);
    sc.tags.retain(|t| t.name != "budget");
    sc.tags_user_removed.push("budget".into());
    sc.tags.push(Tag::new("hardware", TagSource::User));
    meeting::sidecar::write(&f.dir, &sc).unwrap();
    let (r, _) = summarise(&f).await;
    assert_eq!(r["meeting"]["status"], "done", "{r}");
    let got = tags(&f);
    assert!(!got.iter().any(|(n, _)| n == "budget"), "{got:?}");
    assert!(got.contains(&("hardware".into(), TagSource::User)));
    assert!(got.contains(&("gpu".into(), TagSource::Ai)));
}
