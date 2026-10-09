//! F19-FR6 / F19-AC2 / F19-AC4 / EC30 end to end against a mock Anthropic: Summarise with note
//! regions sends them in the one minutes request with the `notes` field, maps the lines read
//! back onto the strokes, and writes `notes-text.json`; more than six regions take more calls.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::notes_text::{NotesInput, NotesSource, RegionImage, StrokeBox};
use meeting::providers::Endpoints;
use meeting::status::Status;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const AN: &str = "sk-ant-SECRET-anthropic-0002";

pub struct Fx {
    pub e: Env,
    pub rt: Runtime,
    pub s: MockServer,
    pub app: Arc<App>,
    pub id: String,
    pub dir: PathBuf,
}

fn png(n: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    b.extend([n; 16]);
    b
}

/// `n` regions, each 100 × 100 world units side by side, and two strokes in region 1 timed
/// 0:12:41 and 0:12:43, plus an untimed one.
pub fn input(n: usize) -> NotesInput {
    NotesInput {
        regions: (0..n)
            .map(|i| RegionImage {
                rect: [i as f64 * 100.0, 0.0, i as f64 * 100.0 + 100.0, 100.0],
                png: png(i as u8),
            })
            .collect(),
        strokes: vec![
            StrokeBox {
                id: 4,
                bbox: [112.0, 20.0, 130.0, 24.0],
                at_ms: Some(761_000),
            },
            StrokeBox {
                id: 5,
                bbox: [135.0, 21.0, 158.0, 25.0],
                at_ms: Some(763_000),
            },
            StrokeBox {
                id: 6,
                bbox: [150.0, 22.0, 155.0, 24.0],
                at_ms: None,
            },
            StrokeBox {
                id: 9,
                bbox: [10.0, 80.0, 20.0, 90.0],
                at_ms: Some(1),
            },
        ],
    }
}

/// An Anthropic key and a transcribed meeting whose notes are `notes`.
pub fn fixture(notes: Option<NotesInput>) -> Fx {
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
    let mut app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    app.notes = NotesSource(Arc::new(move |_, _| notes.clone()));
    parse(&app.set_settings_json(r#"{"summarise":{"provider":"anthropic"}}"#));
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let dir = PathBuf::from(b["firstSegmentPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    app.finish_json(&id, 60_000, "");
    let mut sc = read_sidecar(&dir, &id);
    sc.status = Status::Transcribed;
    sc.segments[0].duration_ms = Some(60_000);
    sc.segments[0].text = Some("We talked about the budget.".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    Fx {
        e,
        rt,
        s,
        app: Arc::new(app),
        id,
        dir,
    }
}

pub fn anthropic(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
        "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 1000, "output_tokens": 10}
    }))
}

/// The minutes reply with `notes`.
pub fn minutes(notes: Value) -> String {
    json!({"title": "Budget", "summary": "S", "decisions": [], "todos": [], "sections": [],
           "notes": notes})
    .to_string()
}

impl Fx {
    pub fn mount(&self, contains: &str, t: ResponseTemplate) {
        let m = Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_string_contains(contains))
            .respond_with(t);
        self.rt.block_on(m.mount(&self.s));
    }
    pub fn run(&self) -> Value {
        parse(&self.app.run_job_json(&self.id, Job::Summarise))
    }
    pub fn requests(&self) -> Vec<Value> {
        let reqs = self.rt.block_on(self.s.received_requests()).unwrap();
        reqs.iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
    pub fn notes_text(&self) -> Value {
        let p = meeting::notes_text::path(&self.dir, &self.id);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }
}

fn images_in(body: &Value) -> usize {
    body["messages"][0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "image")
        .count()
}

/// F19-AC4 and F19-AC2.
#[test]
fn two_regions_go_in_the_one_minutes_request_and_come_back_as_notes_text() {
    let fx = fixture(Some(input(2)));
    let reply = minutes(json!([
        {"region": 1, "lines": [{"text": "Q3 budget ↑", "box": [0.1, 0.2, 0.6, 0.25]},
                                {"text": "unread", "box": [0.9, 0.9, 0.95, 0.95]}]},
        {"region": 7, "lines": [{"text": "no such region", "box": [0, 0, 1, 1]}]}
    ]));
    fx.mount("", anthropic(&reply));
    let out = fx.run();
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let reqs = fx.requests();
    assert_eq!(reqs.len(), 1, "exactly one summarise request");
    assert_eq!(images_in(&reqs[0]), 2);
    let system = reqs[0]["system"][0]["text"].as_str().unwrap();
    assert!(
        system.ends_with(meeting::notes_text::NOTES_RULE),
        "{system}"
    );
    assert!(reqs[0].to_string().contains("Note region 1:"));
    let schema = reqs[0].to_string();
    assert!(schema.contains("\"notes\""), "the notes schema field");
    let t = fx.notes_text();
    assert_eq!(
        t.as_array().unwrap().len(),
        2,
        "every region sent is stored"
    );
    assert_eq!(t[0]["region"], 0);
    assert_eq!(
        t[0]["lines"],
        json!([]),
        "EC30: nothing read, nothing stored"
    );
    let line = &t[1]["lines"][0];
    assert_eq!(line["text"], "Q3 budget ↑");
    assert_eq!(line["box"], json!([110.0, 20.0, 160.0, 25.0]));
    assert_eq!(line["strokeIds"], json!([4, 5, 6]));
    assert_eq!(line["atMs"], 761_000, "the earliest stroke: 0:12:41");
    let unread = &t[1]["lines"][1];
    assert_eq!(
        (unread["strokeIds"].clone(), unread["atMs"].clone()),
        (json!([]), Value::Null)
    );
    let sc = read_sidecar(&fx.dir, &fx.id);
    assert!(
        sc.extra["notesRecognisedAt"]
            .as_str()
            .unwrap()
            .starts_with("20")
    );
    // F16: one usage record, for the one call.
    assert_eq!(sc.usage.len(), 1);
    // And it is searchable, with the line's time and strokes.
    fx.app.index_update_json();
    let r = parse(&fx.app.search_json("q3 budget", "notes", 0));
    let h = &r["groups"][0]["hits"][0];
    assert_eq!(
        (h["time"].as_str(), h["strokeIds"].clone()),
        (Some("0:12:41"), json!([4, 5, 6]))
    );
}

#[test]
fn without_notes_the_request_is_unchanged() {
    let fx = fixture(None);
    fx.mount("", anthropic(&minutes(json!([]))));
    assert_eq!(fx.run()["meeting"]["status"], "done");
    let reqs = fx.requests();
    assert_eq!(images_in(&reqs[0]), 0);
    assert!(!reqs[0].to_string().contains("\"notes\""));
    assert!(!meeting::notes_text::path(&fx.dir, &fx.id).exists());
    assert!(
        read_sidecar(&fx.dir, &fx.id)
            .extra
            .get("notesRecognisedAt")
            .is_none()
    );
}

/// Regions past six go in a further notes-only call, recorded like any other.
#[test]
fn more_than_six_regions_take_a_second_call() {
    let fx = fixture(Some(input(8)));
    fx.mount(
        "Note region 6:",
        anthropic(
            &json!({"notes": [{"region": 7, "lines": [{"text": "late", "box": [0, 0, 1, 1]}]}]})
                .to_string(),
        ),
    );
    fx.mount(
        "Transcript:",
        anthropic(&minutes(
            json!([{"region": 0, "lines": [{"text": "first", "box": [0, 0, 1, 1]}]}]),
        )),
    );
    assert_eq!(fx.run()["meeting"]["status"], "done");
    let reqs = fx.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(images_in(&reqs[0]), 6);
    assert_eq!(images_in(&reqs[1]), 2);
    assert!(
        reqs[1]["system"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("You read handwritten notes")
    );
    let t = fx.notes_text();
    assert_eq!(t.as_array().unwrap().len(), 8);
    assert_eq!(t[0]["lines"][0]["text"], "first");
    assert_eq!(t[7]["lines"][0]["text"], "late");
    assert_eq!(t[7]["lines"][0]["box"], json!([700.0, 0.0, 800.0, 100.0]));
    let sc = read_sidecar(&fx.dir, &fx.id);
    assert_eq!(sc.usage.len(), 2, "both calls recorded (F16)");
    assert!(sc.extra.contains_key("notesRecognisedAt"));
}

/// A failed notes-only call keeps the minutes and the first regions, but the canvas isn't
/// recognised up to now, so the next Summarise again sends it again.
#[test]
fn a_failed_extra_call_keeps_the_minutes() {
    let fx = fixture(Some(input(7)));
    fx.mount(
        "Note region 6:",
        ResponseTemplate::new(400).set_body_string("bad"),
    );
    fx.mount("Transcript:", anthropic(&minutes(json!([]))));
    assert_eq!(fx.run()["meeting"]["status"], "done");
    assert_eq!(fx.notes_text().as_array().unwrap().len(), 6);
    let sc = read_sidecar(&fx.dir, &fx.id);
    assert!(!sc.extra.contains_key("notesRecognisedAt"));
}
