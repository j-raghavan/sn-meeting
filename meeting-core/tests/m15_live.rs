//! F15-FR5 / FR7 against a mock OpenAI: the near-live lane sends pending chunks one at a time,
//! in order, with the provider fixed at Record; it backs off on 5xx and catches up in order,
//! pauses on a rejected key, retries an empty reply for speech once, and records usage per chunk.
mod common;
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::App;
use meeting::library::ChunkIn;
use meeting::providers::Endpoints;
use meeting::sidecar::{ChunkState, Sidecar};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Replies in turn: each entry is (status, text); the last one repeats.
struct Script(Vec<(u16, &'static str)>, AtomicUsize);

impl Respond for Script {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.1.fetch_add(1, Ordering::SeqCst).min(self.0.len() - 1);
        let (status, text) = self.0[i];
        if status == 200 {
            ResponseTemplate::new(200)
                .set_body_json(json!({"text": text, "usage": {"type": "duration", "seconds": 30}}))
        } else {
            ResponseTemplate::new(status).set_body_json(json!({"error": {"message": "busy"}}))
        }
    }
}

struct Fx {
    e: Env,
    app: Arc<App>,
    rt: tokio::runtime::Runtime,
    s: MockServer,
}

impl Fx {
    fn new(key_lines: &str) -> Fx {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let s = rt.block_on(MockServer::start());
        let e = Env::new();
        let key_dir = e.data.path().join("keys");
        fs::create_dir_all(&key_dir).unwrap();
        if !key_lines.is_empty() {
            let body = format!(
                "provider=openai\nkey={}\nmodel=gpt-4o-mini\n{key_lines}",
                common::OA
            );
            fs::write(key_dir.join("copilot-key-openai.txt"), body).unwrap();
        }
        let core = Core {
            app_data_dir: e.data.path().into(),
            key_dir,
            endpoints: Endpoints {
                openai: format!("{}/v1", s.uri()),
                gemini: s.uri(),
                anthropic: s.uri(),
            },
            timeout: Duration::from_secs(5),
            https_only: false,
        };
        let app = Arc::new(App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT))));
        begin(&app, T0);
        Fx { e, app, rt, s }
    }

    fn replies(&self, script: Vec<(u16, &'static str)>) {
        self.rt.block_on(
            Mock::given(method("POST"))
                .and(path("/v1/audio/transcriptions"))
                .respond_with(Script(script, AtomicUsize::new(0)))
                .mount(&self.s),
        );
    }

    fn chunk_path(&self, index: u32) -> PathBuf {
        self.e
            .data
            .path()
            .join("chunks")
            .join(ID0)
            .join(format!("c-{index:04}.m4a"))
    }

    /// Chunk `index` (30 s each, back to back) written and reported, with `speech_ms`.
    fn chunk(&self, index: u32, speech_ms: i64) {
        let p = self.chunk_path(index);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"\x00\x00\x00\x18ftypM4A chunk").unwrap();
        let c = ChunkIn {
            index: index as i32,
            path: &p.to_string_lossy(),
            start_offset_ms: i64::from(index) * 30_000,
            duration_ms: 30_000,
            speech_ms,
        };
        parse(&self.app.chunk_ready_json(ID0, c));
    }

    fn drain(&self) -> Value {
        let mut v = parse(&self.app.live_drain_json(ID0));
        common::assert_no_key(&v.to_string());
        v.as_object_mut().unwrap().remove("waiting");
        v
    }

    fn sc(&self) -> Sidecar {
        read_sidecar(&self.e.export(), ID0)
    }

    fn sent(&self) -> Vec<String> {
        let reqs = self.rt.block_on(self.s.received_requests()).unwrap();
        reqs.iter()
            .filter(|r| r.url.path() == "/v1/audio/transcriptions")
            .map(|r| {
                let b = String::from_utf8_lossy(&r.body);
                b.split("filename=\"")
                    .nth(1)
                    .unwrap_or("")
                    .split('"')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }
}

const WHISPER: &str = "transcribe_model=whisper-1\n";

#[test]
fn a_503_backs_off_later_chunks_wait_and_all_end_done_in_order() {
    // F15-AC3, F15-AC6 (host part), F16-AC1.
    let fx = Fx::new(WHISPER);
    assert_eq!(
        parse(&fx.app.start_live_json(ID0, true)),
        json!({"provider": "openai", "model": "whisper-1", "send": true})
    );
    fx.replies(vec![
        (200, "one"),
        (200, "two"),
        (503, ""),
        (503, ""),
        (200, "three"),
        (200, "four"),
        (200, "five"),
        (200, "six"),
    ]);
    for i in 0..6 {
        fx.chunk(i, 25_000);
    }
    let first = parse(&fx.app.live_drain_json(ID0));
    // Offline · 4 waiting: chunk 3 and the three after it.
    assert_eq!(
        first,
        json!({"state": "retry", "retryInMs": 5_000, "done": 2, "waiting": 4})
    );
    let sc = fx.sc();
    let states: Vec<ChunkState> = sc.chunks.iter().map(|c| c.state).collect();
    assert_eq!(
        &states[..3],
        [ChunkState::Done, ChunkState::Done, ChunkState::Pending]
    );
    assert_eq!(
        (sc.chunks[2].attempts, sc.chunks[2].last_error.is_some()),
        (1, true)
    );
    assert_eq!(
        fx.drain(),
        json!({"state": "retry", "retryInMs": 15_000, "done": 0})
    );
    assert_eq!(fx.drain(), json!({"state": "idle", "done": 4}));
    let sc = fx.sc();
    let texts: Vec<&str> = sc
        .chunks
        .iter()
        .map(|c| c.text.as_deref().unwrap())
        .collect();
    assert_eq!(texts, ["one", "two", "three", "four", "five", "six"]);
    assert_eq!(sc.chunks[2].last_error, None);
    // Sent in order; chunk 3 three times; never a later chunk before it.
    assert_eq!(
        fx.sent(),
        [
            "c-0000.m4a",
            "c-0001.m4a",
            "c-0002.m4a",
            "c-0002.m4a",
            "c-0002.m4a",
            "c-0003.m4a",
            "c-0004.m4a",
            "c-0005.m4a"
        ]
    );
    // One usage record per successful call, naming its chunk (failed calls report nothing).
    let usage = json!(sc.usage);
    let chunks: Vec<u64> = usage
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["chunk"].as_u64().unwrap())
        .collect();
    assert_eq!(chunks, [0, 1, 2, 3, 4, 5]);
    assert_eq!(usage[0]["billedSeconds"], 30.0);
    assert_eq!(usage[0]["usd"], 0.003);
    assert_eq!(usage[0]["source"], "provider");
    // Chunk files stay until their segment is closed (F15-FR8), then go.
    assert!(fx.chunk_path(0).exists());
    fx.app.close_segment_json(ID0, 0, 180_000);
    assert!(!fx.chunk_path(0).exists());
}

#[test]
fn a_rejected_key_pauses_the_lane_with_the_reason_and_the_chunks_wait() {
    // EC16: recording carries on; nothing is sent again until the job after Stop.
    let fx = Fx::new(WHISPER);
    fx.app.start_live_json(ID0, true);
    fx.replies(vec![(401, "")]);
    fx.chunk(0, 25_000);
    fx.chunk(1, 25_000);
    let v = fx.drain();
    assert_eq!(v["state"], "paused");
    assert_eq!(
        v["reason"],
        "Transcription paused: OpenAI rejected the key (401)"
    );
    assert_eq!(
        fx.sc().live.unwrap().paused.as_deref(),
        v["reason"].as_str()
    );
    assert_eq!(
        fx.drain(),
        json!({"state": "paused", "reason": v["reason"], "done": 0})
    );
    assert_eq!(fx.sent().len(), 1);
    assert!(
        fx.sc()
            .chunks
            .iter()
            .all(|c| c.state == ChunkState::Pending)
    );
}

#[test]
fn an_empty_reply_for_speech_is_sent_once_more_then_stored_as_no_speech() {
    let fx = Fx::new(WHISPER);
    fx.app.start_live_json(ID0, true);
    fx.replies(vec![(200, ""), (200, ""), (200, "")]);
    fx.chunk(0, 20_000); // speech: retried once
    fx.chunk(1, 2_000); // a pause: stored at once
    assert_eq!(fx.drain(), json!({"state": "idle", "done": 2}));
    assert_eq!(fx.sent(), ["c-0000.m4a", "c-0000.m4a", "c-0001.m4a"]);
    let sc = fx.sc();
    assert_eq!(sc.chunks[0].text.as_deref(), Some("(no speech detected)"));
    assert_eq!((sc.chunks[0].attempts, sc.chunks[1].attempts), (2, 1));
    assert_eq!(sc.usage.len(), 3, "both calls of chunk 0 were billed");
}

#[test]
fn the_lane_runs_only_for_a_near_live_recording_under_way() {
    let fx = Fx::new(WHISPER);
    fx.chunk(0, 25_000);
    assert_eq!(fx.drain(), json!({"state": "off", "done": 0}));
    // Declined notice (F15-FR12): recorded, never sent while recording.
    fx.app.start_live_json(ID0, false);
    let v = fx.drain();
    assert_eq!(
        (v["state"].as_str(), v["reason"].as_str()),
        (Some("paused"), Some(meeting::app::LIVE_DECLINED))
    );
    fx.app.finish_json(ID0, 30_000, "");
    assert_eq!(fx.drain(), json!({"state": "stopped", "done": 0}));
    assert!(
        parse(&fx.app.start_live_json(ID0, true))["error"]
            .as_str()
            .unwrap()
            .contains("not recording")
    );
    assert!(fx.sent().is_empty());
    assert_eq!(
        parse(&fx.app.live_drain_json("bad")),
        json!({"error": "invalid meeting id"})
    );
}

#[test]
fn nothing_pending_is_idle_and_a_missing_file_blocks_until_stop() {
    let fx = Fx::new(WHISPER);
    fx.app.start_live_json(ID0, true);
    fx.replies(vec![(200, "x")]);
    assert_eq!(fx.drain(), json!({"state": "idle", "done": 0}));
    fx.chunk(0, 25_000);
    fs::remove_file(fx.chunk_path(0)).unwrap();
    assert_eq!(fx.drain(), json!({"state": "blocked", "done": 0}));
}

#[test]
fn a_missing_key_pauses_with_how_to_add_it() {
    // EC20: recording works with no key; the chunks wait.
    let fx = Fx::new("");
    let v = parse(&fx.app.start_live_json(ID0, true));
    assert_eq!(
        v,
        json!({"provider": "openai", "model": null, "send": true})
    );
    fx.chunk(0, 25_000);
    let v = fx.drain();
    assert_eq!(v["state"], "paused");
    assert!(
        v["reason"]
            .as_str()
            .unwrap()
            .contains("copilot-key-openai.txt"),
        "{v}"
    );
}

#[test]
fn a_default_model_is_picked_from_the_listing_once_and_kept() {
    // F5-FR12 for the lane: no model chosen at Record; the first chunk picks and stores it.
    let fx = Fx::new("label=work\n");
    assert_eq!(
        parse(&fx.app.start_live_json(ID0, true))["model"],
        json!(null)
    );
    fx.rt.block_on(
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": [{"id": "whisper-1"}, {"id": "gpt-4o-mini-transcribe"}]}),
            ))
            .mount(&fx.s),
    );
    fx.replies(vec![(200, "a")]);
    fx.chunk(0, 25_000);
    fx.chunk(1, 25_000);
    assert_eq!(fx.drain()["done"], 2);
    assert_eq!(
        fx.sc().live.unwrap().model.as_deref(),
        Some("gpt-4o-mini-transcribe")
    );
    let reqs = fx.rt.block_on(fx.s.received_requests()).unwrap();
    assert_eq!(
        reqs.iter().filter(|r| r.url.path() == "/v1/models").count(),
        1
    );
    let usage = json!(fx.sc().usage);
    assert_eq!(usage[1]["model"], "gpt-4o-mini-transcribe");
}

#[test]
fn a_second_caller_is_told_the_lane_is_busy() {
    let fx = Fx::new(WHISPER);
    fx.app.start_live_json(ID0, true);
    fx.rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(600))
                    .set_body_json(json!({"text": "slow"})),
            )
            .mount(&fx.s),
    );
    fx.chunk(0, 25_000);
    let app = fx.app.clone();
    let h = std::thread::spawn(move || app.live_drain_json(ID0));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        parse(&fx.app.live_drain_json(ID0)),
        json!({"state": "busy"})
    );
    assert_eq!(parse(&h.join().unwrap())["done"], 1);
}

#[test]
fn the_live_transcript_appends_whole_lines_and_says_when_it_is_waiting() {
    // F15-FR6 / FR7, F17-FR6, F16-FR5's "≈ $x so far".
    let fx = Fx::new(WHISPER);
    let lt = |after: i32| parse(&fx.app.live_transcript_json(ID0, after));
    assert_eq!(
        lt(0),
        json!({"lines": [], "waiting": 0, "state": "off", "spentUsd": 0.0, "spent": "—"})
    );
    fx.app.start_live_json(ID0, true);
    fx.replies(vec![
        (200, "And then we ship it on Friday."),
        (503, ""),
        (200, "ship it on Friday. Next, the budget."),
    ]);
    fx.chunk(0, 25_000);
    // Chunk 1 is a hard cut's: it starts 1 s before chunk 0 ends (30 s chunks, 29 s apart).
    let p = fx.chunk_path(1);
    fs::write(&p, b"\x00\x00\x00\x18ftypM4A chunk").unwrap();
    let c = ChunkIn {
        index: 1,
        path: &p.to_string_lossy(),
        start_offset_ms: 29_000,
        duration_ms: 30_000,
        speech_ms: 25_000,
    };
    parse(&fx.app.chunk_ready_json(ID0, c));
    assert_eq!(lt(0)["state"], "transcribing");
    fx.drain(); // chunk 0 done, chunk 1 fails
    let v = lt(0);
    assert_eq!(
        (v["state"].as_str(), v["waiting"].as_u64()),
        (Some("offline"), Some(1))
    );
    assert_eq!(
        v["lines"],
        json!([{"n": 1, "atMs": 0, "text": "And then we ship it on Friday."}])
    );
    assert_eq!(v["spent"], "< $0.01");
    fx.drain();
    let v = lt(0);
    assert_eq!(v["state"], "current");
    // The repeated words appear once; the line's time is estimated after the overlap.
    assert_eq!(
        v["lines"][1],
        json!({"n": 2, "atMs": 30_000, "text": "Next, the budget."})
    );
    assert_eq!(lt(1)["lines"].as_array().unwrap().len(), 1, "after line 1");
    assert_eq!(v["spentUsd"], 0.006);
    // Closing the segment fills its text from the chunks (F15-FR6), as F5 reads it.
    fx.app.finish_json(ID0, 59_000, "");
    fx.app.close_segment_json(ID0, 0, 59_000);
    assert_eq!(
        fx.sc().segments[0].text.as_deref(),
        Some("And then we ship it on Friday.\n\nNext, the budget.")
    );
    let m = parse(&fx.app.get_meeting_json(ID0));
    assert_eq!(
        m["paragraphs"][1],
        json!({"index": 1, "atMs": 29_000, "text": "Next, the budget."})
    );
    // Paused while recording shows its reason.
    let fx = Fx::new(WHISPER);
    fx.app.start_live_json(ID0, false);
    let v = parse(&fx.app.live_transcript_json(ID0, 0));
    assert_eq!(
        (v["state"].as_str(), v["reason"].as_str()),
        (Some("paused"), Some(meeting::app::LIVE_DECLINED))
    );
    assert!(parse(&fx.app.live_transcript_json("meeting-20991231-235959", 0))["error"].is_string());
}
