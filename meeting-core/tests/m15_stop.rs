//! F15-FR9 / D15 against a mock OpenAI: at Stop the meeting is `recorded`; the Stop job sends
//! only the chunks still pending (never the done ones again, INV11), under the job rules, then
//! fills the minutes with one summarise call. Chunk files gone are re-cut from their segment
//! first; recovery lists near-live meetings whose catch-up must resume.
mod common;
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::library::ChunkIn;
use meeting::providers::Endpoints;
use meeting::sidecar::{Chunk, ChunkState, Sidecar};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SUMMARY: &str = r#"{"title":"Budget","summary":"S","decisions":[],"todos":[],"sections":[]}"#;

struct Fx {
    e: Env,
    app: Arc<App>,
    rt: tokio::runtime::Runtime,
    s: MockServer,
}

impl Fx {
    fn new() -> Fx {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let s = rt.block_on(MockServer::start());
        let e = Env::new();
        let key_dir = e.data.path().join("keys");
        fs::create_dir_all(&key_dir).unwrap();
        let body = format!(
            "provider=openai\nkey={}\nmodel=gpt-4o-mini\ntranscribe_model=whisper-1\n",
            common::OA
        );
        fs::write(key_dir.join("copilot-key-openai.txt"), body).unwrap();
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
        app.start_live_json(ID0, true);
        let fx = Fx { e, app, rt, s };
        fx.mount("/v1/responses", ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "status": "completed", "model": "gpt-4o-mini",
            "output": [{"type": "message", "id": "m", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": SUMMARY, "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}})));
        fx
    }

    fn mount(&self, p: &str, r: ResponseTemplate) {
        self.rt.block_on(
            Mock::given(method("POST"))
                .and(path(p))
                .respond_with(r)
                .mount(&self.s),
        );
    }

    fn transcribes(&self, text: &str) {
        self.mount(
            "/v1/audio/transcriptions",
            ResponseTemplate::new(200).set_body_json(json!({"text": text})),
        );
    }

    fn chunk_path(&self, i: u32) -> PathBuf {
        self.e
            .data
            .path()
            .join("chunks")
            .join(ID0)
            .join(format!("c-{i:04}.m4a"))
    }

    /// `done` chunks already transcribed while recording, then `pending` more with files.
    fn chunks(&self, done: u32, pending: u32) {
        let mut sc = self.sc();
        for i in 0..done {
            sc.chunks.push(Chunk {
                index: i,
                seg_index: 0,
                start_offset_ms: u64::from(i) * 30_000,
                duration_ms: 30_000,
                overlap_ms: 0,
                state: ChunkState::Done,
                text: Some(format!("line {i}.")),
                attempts: 1,
                last_error: None,
                speech_ms: Some(20_000),
                extra: Default::default(),
            });
        }
        meeting::sidecar::write(&self.e.export(), &sc).unwrap();
        for i in done..done + pending {
            let p = self.chunk_path(i);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"\x00\x00\x00\x18ftypM4A chunk").unwrap();
            let c = ChunkIn {
                index: i as i32,
                path: &p.to_string_lossy(),
                start_offset_ms: i64::from(i) * 30_000,
                duration_ms: 30_000,
                speech_ms: 20_000,
            };
            parse(&self.app.chunk_ready_json(ID0, c));
        }
    }

    /// Stop: the last chunk is in, the meeting finished and its segment closed.
    fn stop(&self, total_ms: i64) {
        fs::write(self.e.export().join(format!("{ID0}-01.m4a")), b"segment").unwrap();
        self.app.finish_json(ID0, total_ms, "");
        self.app.close_segment_json(ID0, 0, total_ms);
    }

    fn sc(&self) -> Sidecar {
        read_sidecar(&self.e.export(), ID0)
    }

    fn requests(&self, p: &str) -> Vec<String> {
        let reqs = self.rt.block_on(self.s.received_requests()).unwrap();
        reqs.iter()
            .filter(|r| r.url.path() == p)
            .map(|r| String::from_utf8_lossy(&r.body).into_owned())
            .collect()
    }
}

#[test]
fn at_stop_only_the_pending_chunks_are_sent_then_the_minutes_once() {
    // F15-AC4, INV11, D15.
    let fx = Fx::new();
    fx.transcribes("the last words.");
    fx.chunks(40, 2);
    let mark = parse(&fx.app.add_mark_json(ID0, 761_000));
    fx.app
        .set_mark_label_json(ID0, mark["markId"].as_str().unwrap(), "Budget decision");
    fx.stop(42 * 30_000);
    assert_eq!(fx.sc().status, meeting::status::Status::Recorded);
    let app = fx.app.clone();
    let out = parse(&app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let sent = fx.requests("/v1/audio/transcriptions");
    assert_eq!(sent.len(), 2, "never the 40 done ones again");
    assert!(sent[0].contains("c-0040.m4a") && sent[1].contains("c-0041.m4a"));
    let summaries = fx.requests("/v1/responses");
    assert_eq!(summaries.len(), 1, "the minutes are filled once");
    assert!(summaries[0].contains("line 39.") && summaries[0].contains("the last words."));
    // F15-FR11: the marks go with the one summarise call.
    assert!(
        summaries[0].contains("The user marked these moments:")
            && summaries[0].contains("[0:12:41] Budget decision")
    );
    let sc = fx.sc();
    assert!(sc.chunks.iter().all(|c| c.state == ChunkState::Done));
    assert_eq!(sc.providers.transcribe.as_ref().unwrap().model, "whisper-1");
    // The segment's text is filled from its chunks, so F5's fields read as before.
    assert!(
        sc.segments[0]
            .text
            .as_deref()
            .unwrap()
            .starts_with("line 0.\n\nline 1.")
    );
    // The transcript file has a paragraph per chunk, each with its marker.
    let md = fs::read_to_string(fx.e.export().join(format!("{ID0}.transcript.md"))).unwrap();
    assert!(
        md.contains("0:20:30") && md.contains("the last words."),
        "{md}"
    );
    // Every chunk file is gone now (text stored, segment closed, F15-FR8).
    assert!(!fx.e.data.path().join("chunks").join(ID0).exists());
}

#[test]
fn with_every_chunk_done_at_stop_nothing_is_transcribed_again() {
    let fx = Fx::new();
    fx.chunks(3, 0);
    fx.stop(90_000);
    let out = parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(out["meeting"]["status"], "done");
    assert!(fx.requests("/v1/audio/transcriptions").is_empty());
    assert_eq!(fx.requests("/v1/responses").len(), 1);
}

#[test]
fn a_chunk_that_keeps_failing_fails_the_job_and_a_retry_resumes_there() {
    // EC14 / EC15 after Stop: one automatic retry, then failed (transcribing), retryable.
    let fx = Fx::new();
    fx.chunks(1, 2);
    fx.stop(90_000);
    fx.rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(503).set_body_json(json!({"error": {"message": "busy"}})),
            )
            .up_to_n_times(2)
            .with_priority(1)
            .mount(&fx.s),
    );
    fx.transcribes("back.");
    let out = parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(
        (
            out["meeting"]["status"].as_str(),
            out["meeting"]["failedAt"].as_str()
        ),
        (Some("failed"), Some("transcribing"))
    );
    assert!(
        out["meeting"]["error"].as_str().unwrap().contains("503"),
        "{out}"
    );
    assert_eq!(fx.requests("/v1/audio/transcriptions").len(), 2);
    let out = parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(out["meeting"]["status"], "done");
    let sc = fx.sc();
    assert_eq!(sc.chunks[1].attempts, 3);
    assert_eq!(
        fx.requests("/v1/audio/transcriptions").len(),
        4,
        "chunk 1 again, then chunk 2"
    );
}

#[test]
fn a_gone_chunk_is_listed_to_recut_and_its_audio_missing_once_it_cant_be() {
    let fx = Fx::new();
    fx.transcribes("words.");
    fx.chunks(1, 2);
    fx.stop(90_000);
    fs::remove_file(fx.chunk_path(1)).unwrap();
    let cuts = parse(&fx.app.chunks_to_cut_json(ID0));
    let seg = fx.e.export().join(format!("{ID0}-01.m4a"));
    assert_eq!(
        cuts,
        json!([{"index": 1, "segIndex": 0, "fromMs": 30_000, "toMs": 60_000, "segPath": seg,
                "path": fx.chunk_path(1), "startOffsetMs": 30_000, "durationMs": 30_000}])
    );
    // The shell's re-cut failed: that audio can't be read, so it is "(audio missing)" (EC7).
    let out = parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let sc = fx.sc();
    assert_eq!(sc.chunks[1].state, ChunkState::Missing);
    assert_eq!(fx.requests("/v1/audio/transcriptions").len(), 1);
    assert!(
        sc.segments[0]
            .text
            .as_deref()
            .unwrap()
            .contains("(audio missing)")
    );
    // Transcribed: Transcribe again would send every chunk, so all of them are listed now.
    let cuts = parse(&fx.app.chunks_to_cut_json(ID0));
    assert_eq!(cuts.as_array().unwrap().len(), 3);
    assert!(parse(&fx.app.chunks_to_cut_json("meeting-20991231-235959"))["error"].is_string());
}

#[test]
fn a_gone_chunk_of_a_segment_not_yet_closed_fails_and_waits() {
    let fx = Fx::new();
    fx.chunks(0, 1);
    fx.app.finish_json(ID0, 30_000, "");
    fs::remove_file(fx.chunk_path(0)).unwrap();
    assert_eq!(
        parse(&fx.app.chunks_to_cut_json(ID0)),
        json!([]),
        "its segment isn't closed"
    );
    let out = parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    assert_eq!(out["meeting"]["status"], "failed");
    assert_eq!(
        out["meeting"]["error"],
        "Chunk 1 of the transcript can't be read yet"
    );
}

#[test]
fn recovery_resumes_the_catch_up_of_a_near_live_meeting() {
    // F15-AC8 (host part): killed mid-recording; on relaunch the meeting is recorded and its
    // pending chunks are named so the shell runs the Stop job.
    let fx = Fx::new();
    fx.chunks(1, 1);
    let r = parse(&fx.app.recover_json(false));
    assert_eq!(
        (r["recovered"].clone(), r["resume"].clone()),
        (json!([ID0]), json!([ID0]))
    );
    assert_eq!(
        parse(&fx.app.recover_json(false)).get("resume"),
        None,
        "only once"
    );
    // Declined (F15-FR12): recovered, but the chunks wait for Transcribe.
    let fx = Fx::new();
    fx.app.start_live_json(ID0, false);
    fx.chunks(0, 1);
    let r: Value = parse(&fx.app.recover_json(false));
    assert_eq!(
        (r["recovered"].clone(), r.get("resume")),
        (json!([ID0]), None)
    );
}

#[test]
fn transcribe_again_sends_every_chunk_over_its_recorded_range() {
    // F15-FR10: chunk texts are cleared and every chunk re-sent (re-cut by the shell first);
    // the to-dos and title stay; line times stay aligned because the ranges don't change.
    let fx = Fx::new();
    fx.transcribes("again.");
    fx.chunks(3, 0);
    fx.stop(90_000);
    parse(&fx.app.run_job_json(ID0, Job::Transcribe));
    let before: Vec<u64> = fx.sc().chunks.iter().map(|c| c.start_offset_ms).collect();
    // The shell re-cuts every chunk (nativeChunksToCut lists them all once transcribed).
    let cuts = parse(&fx.app.chunks_to_cut_json(ID0));
    for c in cuts.as_array().unwrap() {
        let p = PathBuf::from(c["path"].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"\x00\x00\x00\x18ftypM4A recut").unwrap();
        let ci = ChunkIn {
            index: c["index"].as_i64().unwrap() as i32,
            path: &p.to_string_lossy(),
            start_offset_ms: c["startOffsetMs"].as_i64().unwrap(),
            duration_ms: c["durationMs"].as_i64().unwrap(),
            speech_ms: -1,
        };
        assert_eq!(parse(&fx.app.chunk_ready_json(ID0, ci))["added"], false);
    }
    let out = parse(&fx.app.run_job_json(ID0, Job::TranscribeAgain));
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let sc = fx.sc();
    assert_eq!(fx.requests("/v1/audio/transcriptions").len(), 3);
    assert!(
        sc.chunks
            .iter()
            .all(|c| c.text.as_deref() == Some("again."))
    );
    assert_eq!(
        sc.chunks
            .iter()
            .map(|c| c.start_offset_ms)
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(sc.title.as_deref(), Some("Budget"));
    assert_eq!(fx.requests("/v1/responses").len(), 2);
}
