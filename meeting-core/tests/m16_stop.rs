//! F21-AC15 (host, mock server) and F21-FR15: typed text is searchable at its time with no
//! provider call; the Stop call carries the strokes' region images and the typed-notes block,
//! and never an inserted image.
mod libcommon;

use base64::Engine;
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

const AN: &str = "sk-ant-SECRET-anthropic-0003";
const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 0xAB, 0xCD, 0xEF, 0xFF, 0xD9,
];

struct Fx {
    _e: Env,
    rt: Runtime,
    s: MockServer,
    app: Arc<App>,
    id: String,
    dir: PathBuf,
}

fn fixture() -> Fx {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
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
    parse(&app.set_settings_json(
        r#"{"summarise":{"provider":"anthropic"},"notes":{"noticeShown":{"anthropic":true}}}"#,
    ));
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    Fx {
        _e: e,
        rt,
        s,
        app: Arc::new(app),
        id,
        dir,
    }
}

fn edit(f: &Fx, op: Value) -> Value {
    parse(&f.app.canvas_edit_json(&f.id, &op.to_string()))
}

#[test]
fn typed_text_is_timed_and_sent_as_text_and_the_photo_never_goes() {
    let f = fixture();
    // While recording: strokes in two regions far apart, a text box at 0:12:41, a photo.
    for (x, y, t) in [(0.0, 0.0, 700_000.0), (5_000.0, 5_000.0, 800_000.0)] {
        let pts = [x, y, 0.5, t, x + 80.0, y, 0.5, t + 10.0];
        assert!(
            parse(&f.app.ink_stroke_json(&f.id, "canvas", 0, 3.0, &pts))["strokeId"].is_number()
        );
    }
    let r = edit(
        &f,
        json!({"op": "create", "type": "text", "at": [0, 400], "text": "Budget owner?",
                            "height": 40.0, "recMs": 761_000}),
    );
    let text_ref = r["created"].as_str().unwrap().to_owned();
    let src = tempfile::tempdir().unwrap();
    let photo = src.path().join("whiteboard.jpg");
    std::fs::write(&photo, JPEG).unwrap();
    let view = r#"{"rect": [2000, 2000, 3000, 3000]}"#;
    assert!(
        parse(
            &f.app
                .canvas_import_image_json(&f.id, photo.to_str().unwrap(), 400, 300, view)
        )["itemId"]
            .is_string()
    );
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    f.app.finish_json(&f.id, 900_000, "");
    let mut sc = read_sidecar(&f.dir, &f.id);
    sc.status = Status::Transcribed;
    sc.segments[0].duration_ms = Some(900_000);
    sc.segments[0].text = Some("We talked about the budget.".into());
    meeting::sidecar::write(&f.dir, &sc).unwrap();

    // Search finds the typed text at its own time, before any provider call.
    f.app.index_update_json();
    assert!(f.rt.block_on(f.s.received_requests()).unwrap().is_empty());
    let hits = parse(&f.app.search_json("owner", "notes", 0));
    let hit = &hits["groups"][0]["hits"][0];
    assert_eq!(hit["unitId"], format!("t:1:{}", &text_ref[2..]), "{hits}");
    assert_eq!(hit["atMs"], 761_000);
    assert!(hit.get("canvas").is_none(), "the first canvas");

    // Stop's summarise: one request, two region images, the typed block, no photo.
    let reply = json!({"title": "Budget", "summary": "S", "decisions": [], "todos": [],
                       "sections": [], "notes": []})
    .to_string();
    let t = ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
        "content": [{"type": "text", "text": reply}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 1000, "output_tokens": 10}}));
    f.rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(t)
            .mount(&f.s),
    );
    let out = parse(&f.app.run_job_json(&f.id, Job::Summarise));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let reqs = f.rt.block_on(f.s.received_requests()).unwrap();
    assert_eq!(reqs.len(), 1, "one summarise request");
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.iter().filter(|c| c["type"] == "image").count(), 2);
    let all_text = body.to_string();
    assert!(
        all_text.contains("The user's typed notes:\\n- Budget owner?"),
        "{all_text}"
    );
    let b64 = base64::engine::general_purpose::STANDARD.encode(JPEG);
    assert!(
        !all_text.contains(&b64),
        "the inserted photo is never sent (INV20)"
    );
    assert!(!all_text.contains("whiteboard"));
}

/// F21-FR15 / AC12 (search part): typed text and note lines on canvas 2 name it, so a Notes
/// hit opens canvas 2.
#[test]
fn a_hit_on_canvas_two_names_it() {
    let f = fixture();
    f.app.canvas_new_json(&f.id);
    edit(
        &f,
        json!({"op": "create", "type": "note", "at": [0, 0], "text": "Risks", "height": 30.0}),
    );
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    f.app.index_update_json();
    let hits = parse(&f.app.search_json("risks", "notes", 0));
    assert_eq!(hits["groups"][0]["hits"][0]["canvas"], 2, "{hits}");
    assert!(
        hits["groups"][0]["hits"][0]["unitId"]
            .as_str()
            .unwrap()
            .starts_with("t:2:")
    );
}
