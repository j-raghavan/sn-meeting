//! F17-FR9 / F17-AC6 against a mock Anthropic: at Stop's summarise, one request carries the
//! transcript, the marks and the canvas's regions as images (the real canvas source, not a
//! stub), and the minutes fill in. With "Use my notes for the minutes" off, no image goes.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::providers::Endpoints;
use meeting::status::Status;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fx {
    _e: Env,
    rt: Runtime,
    s: MockServer,
    app: App,
    id: String,
    dir: PathBuf,
}

/// A recording with 3 marks and notes in two far-apart places, stopped and transcribed.
fn fixture() -> Fx {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    std::fs::write(
        key_dir.join("copilot-key-anthropic.txt"),
        "provider=anthropic\nkey=sk-ant-SECRET-0003\nmodel=claude-x\n",
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
    parse(&app.set_settings_json(r#"{"summarise":{"provider":"anthropic"}}"#));
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    for at in [61_000, 120_000, 761_000] {
        app.add_mark_json(&id, at);
    }
    // Two words near the top left, and a note 3 000 units below: two regions.
    for (x, y, t) in [
        (0.0f32, 0.0f32, 60_000.0f32),
        (60.0, 0.0, 61_000.0),
        (0.0, 3_000.0, 700_000.0),
    ] {
        let pts = [x, y, 1.0, t, x + 40.0, y + 5.0, 1.0, t + 20.0];
        assert!(
            parse(&app.ink_stroke_json(&id, "canvas", 0, 3.0, &pts))
                .get("error")
                .is_none()
        );
    }
    app.finish_json(&id, 900_000, "");
    let mut sc = read_sidecar(&dir, &id);
    sc.status = Status::Transcribed;
    sc.segments[0].duration_ms = Some(900_000);
    sc.segments[0].text = Some("We talked about the budget.".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    let reply =
        json!({"title": "Budget", "summary": "We agreed the budget.", "decisions": ["Ship"],
                       "todos": [], "sections": [], "notes": []})
        .to_string();
    let m = Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
            "content": [{"type": "text", "text": reply}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 1000, "output_tokens": 10}})));
    rt.block_on(m.mount(&s));
    Fx {
        _e: e,
        rt,
        s,
        app,
        id,
        dir,
    }
}

fn requests(fx: &Fx) -> Vec<Value> {
    let reqs = fx.rt.block_on(fx.s.received_requests()).unwrap();
    reqs.iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn images(body: &Value) -> Vec<Value> {
    body["messages"][0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "image")
        .cloned()
        .collect()
}

/// F17-AC6.
#[test]
fn one_stop_request_carries_the_transcript_the_marks_and_two_note_images() {
    let fx = fixture();
    let out = parse(&fx.app.run_job_json(&fx.id, Job::Summarise));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    assert_eq!(out["meeting"]["summary"], "We agreed the budget.");
    let reqs = requests(&fx);
    assert_eq!(reqs.len(), 1, "exactly one summarise request");
    let body = reqs[0].to_string();
    assert!(
        body.contains("We talked about the budget."),
        "the transcript"
    );
    for m in ["[0:01:01]", "[0:02:00]", "[0:12:41]"] {
        assert!(body.contains(m), "mark {m}");
    }
    let imgs = images(&reqs[0]);
    assert_eq!(imgs.len(), 2, "two canvas regions");
    for i in &imgs {
        let data = i["source"]["data"].as_str().unwrap();
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let (w, h) = meeting::app::png_size(&png).unwrap();
        assert!(w <= 1568 && h <= 1568);
    }
    // The ink is never touched by a provider result (INV12).
    let canvas = meeting::notes::path(&fx.dir, &fx.id, meeting::notes::Layer::Canvas);
    assert_eq!(
        meeting::notes::load(&fx.dir, &fx.id, meeting::notes::Layer::Canvas)
            .unwrap()
            .strokes()
            .len(),
        3
    );
    assert!(canvas.exists());
}

#[test]
fn with_the_setting_off_no_note_goes() {
    let fx = fixture();
    parse(
        &fx.app
            .set_settings_json(r#"{"notes":{"useForMinutes":false}}"#),
    );
    let out = parse(&fx.app.run_job_json(&fx.id, Job::Summarise));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let reqs = requests(&fx);
    assert_eq!(reqs.len(), 1);
    assert!(images(&reqs[0]).is_empty());
}
