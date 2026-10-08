//! F5-FR4/FR5/FR8 end to end against a mock server: transcribe then summarise to `done`, the
//! transcript file from the General layout, Summarise again (to-dos and a user's title kept),
//! the model-rejected fallback, and failures that keep the previous transcript file.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::providers::Endpoints;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const SUMMARY: &str = r#"{"title":"Deck review.","summary":"We reviewed the deck.","decisions":["Ship it"],"todos":[{"text":"Send the deck to Priya","owner":"me","due":"2026-10-09"}],"sections":[{"heading":"Blockers","body":"- None"}]}"#;

struct Fx {
    e: Env,
    rt: Runtime,
    s: MockServer,
    app: Arc<App>,
    id: String,
    dir: PathBuf,
}

/// Keys for OpenAI (transcribe) and Anthropic (summarise, model `claude-x`), and a recorded
/// meeting with segments at 0 and 15:00.
fn fixture(keys: &[&str]) -> Fx {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    for k in keys {
        let body = match *k {
            "openai" => format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\n"),
            _ => format!("provider=anthropic\nkey={AN}\nmodel=claude-x\n"),
        };
        std::fs::write(key_dir.join(format!("copilot-key-{k}.txt")), body).unwrap();
    }
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
    if keys.contains(&"anthropic") {
        let r = parse(&app.set_settings_json(r#"{"summarise":{"provider":"anthropic"}}"#));
        assert!(r.get("error").is_none(), "{r}");
    }
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    let dir = first.parent().unwrap().to_path_buf();
    std::fs::write(&first, b"seg1").unwrap();
    let second = dir.join(format!("{id}-02.m4a"));
    std::fs::write(&second, b"seg2").unwrap();
    app.add_segment_json(&id, second.to_str().unwrap(), 900_000);
    app.finish_json(&id, 2_400_000, "");
    for n in 1..=2 {
        let m = Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .and(body_string_contains(format!("-0{n}.m4a")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"text": format!("text {n}")})),
            );
        rt.block_on(m.mount(&s));
    }
    Fx {
        e,
        rt,
        s,
        app: Arc::new(app),
        id,
        dir,
    }
}

fn anthropic(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-x",
        "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 1, "output_tokens": 1}
    }))
}

impl Fx {
    fn summary(&self, t: ResponseTemplate) {
        let m = Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(t);
        self.rt.block_on(m.mount(&self.s));
    }
    fn reset(&self) {
        self.rt.block_on(self.s.reset());
    }
    fn run(&self, job: Job) -> Value {
        parse(&self.app.run_job_json(&self.id, job))
    }
    fn meeting(&self) -> Value {
        json!(read_sidecar(&self.dir, &self.id))
    }
    fn transcript(&self) -> String {
        let name = format!("{}.transcript.md", self.id);
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
    fn requests(&self, p: &str) -> Vec<Value> {
        let reqs = self.rt.block_on(self.s.received_requests()).unwrap();
        reqs.iter()
            .filter(|r| r.url.path() == p)
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
}

#[test]
fn transcribe_then_summarise_writes_the_transcript_file() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    let out = fx.run(Job::Transcribe);
    let m = &out["meeting"];
    assert_eq!(m["status"], "done", "{out}");
    assert_eq!(
        (m["title"].as_str(), m["titleSource"].as_str()),
        (Some("Deck review"), Some("auto"))
    );
    assert_eq!(m["templateId"], "general");
    assert_eq!(
        m["providers"]["summarise"],
        json!({"provider": "anthropic", "model": "claude-x"})
    );
    assert_eq!(m["todos"][0]["due"], "2026-10-09");
    assert_eq!(m["todos"][0]["id"].as_str().unwrap().len(), 16);
    assert_eq!(
        m["todos"][0]["firstSeen"].as_str().unwrap().len(),
        25,
        "RFC 3339 to the second"
    );
    assert_eq!(m["transcriptFile"], format!("{}.transcript.md", fx.id));
    let md = fx.transcript();
    assert!(
        md.starts_with("# Deck review\nWednesday 2026-10-07 · 21:17 · 40 min · General meeting\n"),
        "{md}"
    );
    for want in [
        "## Summary\nWe reviewed the deck.\n",
        "## Decisions\n- Ship it\n",
        "## Blockers\n- None\n",
        "- [ ] Send the deck to Priya — me (due 2026-10-09)\n",
        "## Transcript\n[0:00:00] text 1\n[0:15:00] text 2\n",
    ] {
        assert!(md.contains(want), "missing {want:?} in\n{md}");
    }
    let asks = fx.requests("/v1/messages");
    assert_eq!(asks.len(), 1, "the whole transcript in one request");
    let system = asks[0]["system"][0]["text"].as_str().unwrap();
    assert!(
        system.starts_with("You are summarising a meeting"),
        "template instructions first"
    );
    assert!(system.contains("The meeting took place on Wednesday 2026-10-07"));
    assert!(system.ends_with(meeting::summarise::FIXED_RULES));
    let user = asks[0]["messages"][0]["content"].to_string();
    assert!(user.contains("[0:00:00] text 1") && user.contains("[0:15:00] text 2"));
    // No templates folder exists: the built-in was used (D10).
    assert!(!fx.e.root.path().join("MyStyle/Meetings").exists());
}

#[test]
fn summarise_again_keeps_ticked_todos_and_a_users_title() {
    // F5-FR8, F5-AC5, F5-AC11; to-dos ticked while the request is in flight survive.
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    let todo = fx.meeting()["todos"][0]["id"].as_str().unwrap().to_owned();
    fx.app.rename_json(&fx.id, "My title");
    fx.reset();
    let reworded = SUMMARY
        .replace("Send the deck to Priya", "send the deck to priya!")
        .replace("Deck review.", "Another title");
    fx.summary(anthropic(&reworded).set_delay(Duration::from_millis(400)));
    let (app, id) = (fx.app.clone(), fx.id.clone());
    let h = std::thread::spawn(move || app.run_job_json(&id, Job::Summarise));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        parse(&fx.app.set_todo_done_json(&fx.id, &todo, true)),
        json!({"ok": true})
    );
    let out = parse(&h.join().unwrap());
    let m = &out["meeting"];
    assert_eq!(m["status"], "done", "{out}");
    assert_eq!(
        (m["title"].as_str(), m["titleSource"].as_str()),
        (Some("My title"), Some("user"))
    );
    assert_eq!(m["todos"].as_array().unwrap().len(), 1);
    assert_eq!(
        (
            m["todos"][0]["id"].as_str(),
            m["todos"][0]["done"].as_bool()
        ),
        (Some(todo.as_str()), Some(true))
    );
    assert!(fx.transcript().contains("- [x] Send the deck to Priya"));
    assert!(fx.transcript().starts_with("# My title\n"));
    assert!(
        fx.requests("/v1/audio/transcriptions").is_empty(),
        "no new transcription"
    );
}

#[test]
fn a_rejected_model_is_retried_once_with_the_default() {
    // F4-FR3.
    let fx = fixture(&["openai", "anthropic"]);
    let rejected = r#"{"type":"error","error":{"type":"invalid_request_error","message":"model: claude-x does not support output_config"}}"#;
    let m = Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("\"claude-x\""))
        .respond_with(ResponseTemplate::new(400).set_body_string(rejected));
    fx.rt.block_on(m.mount(&fx.s));
    fx.summary(anthropic(SUMMARY));
    let out = fx.run(Job::Transcribe);
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    let ran = &out["meeting"]["providers"]["summarise"];
    assert_eq!(
        *ran,
        json!({"provider": "anthropic", "model": "claude-opus-5-5"})
    );
    let models: Vec<Value> = fx
        .requests("/v1/messages")
        .iter()
        .map(|b| b["model"].clone())
        .collect();
    assert_eq!(models, [json!("claude-x"), json!("claude-opus-5-5")]);
}

#[test]
fn failures_keep_the_previous_transcript_and_offer_summarise_again() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    let before = fx.transcript();
    for (reply, want) in [
        (anthropic("not json"), "The summary wasn't valid"),
        (
            ResponseTemplate::new(413),
            "Transcript too large for Anthropic",
        ),
        (
            ResponseTemplate::new(401).set_body_string(AN),
            "Anthropic rejected the key (401)",
        ),
        (
            ResponseTemplate::new(400).set_body_string("model"),
            "HTTP 400: model",
        ),
    ] {
        fx.reset();
        fx.summary(reply);
        let m = fx.run(Job::Summarise)["meeting"].clone();
        assert_eq!(
            (m["status"].as_str(), m["failedAt"].as_str()),
            (Some("failed"), Some("summarising"))
        );
        assert!(m["error"].as_str().unwrap().starts_with(want), "{m}");
        assert_eq!(fx.transcript(), before, "the old transcript file is kept");
        assert!(!m.to_string().contains(AN));
    }
    // Retry from failed(summarising) succeeds.
    fx.reset();
    fx.summary(anthropic(SUMMARY));
    assert_eq!(fx.run(Job::Summarise)["meeting"]["status"], "done");
    assert!(fx.meeting()["error"].is_null());
}

#[test]
fn a_missing_template_falls_back_to_general_with_a_note() {
    // F11-FR6.
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.note = Some("mic disconnected".into());
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    // A YOURS template chosen for the meeting, then deleted.
    let mut t = parse(&fx.app.get_template_json("standup"));
    t["id"] = json!("gone");
    assert_eq!(
        parse(&fx.app.save_template_json(&t.to_string()))["origin"],
        "yours"
    );
    assert_eq!(
        parse(&fx.app.set_template_json(&fx.id, "gone")),
        json!({"ok": true})
    );
    fx.app.reset_template_json("gone");
    assert_eq!(
        parse(&fx.app.set_template_json(&fx.id, "gone")),
        json!({"error": "template not found"})
    );
    let m = fx.run(Job::Transcribe)["meeting"].clone();
    assert_eq!(m["status"], "done");
    assert_eq!(m["templateId"], "general");
    assert_eq!(
        m["note"],
        "mic disconnected; Template gone was not found; used General meeting"
    );
}

#[test]
fn without_a_summarise_key_transcription_stops_at_transcribed() {
    // F7-FR2: Transcribe needs only the transcribe key.
    let fx = fixture(&["openai", "anthropic"]);
    std::fs::remove_file(fx.e.data.path().join("keys/copilot-key-anthropic.txt")).unwrap();
    let m = fx.run(Job::Transcribe)["meeting"].clone();
    assert_eq!(m["status"], "transcribed");
    assert_eq!(
        m["error"],
        "Add copilot-key-anthropic.txt to MyStyle/SnCopilot"
    );
    assert_eq!(
        fx.run(Job::Summarise),
        json!({"error": "Add copilot-key-anthropic.txt to MyStyle/SnCopilot"})
    );
}

#[test]
fn a_render_error_keeps_the_previous_file_and_fails_summarising() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    let before = fx.transcript();
    let mut plan = fx.app.plan(Job::Summarise).unwrap();
    plan.templates[0].layout = "# {{ title }}\n{{ 1 // 0 }}\n".into();
    let lock = std::sync::Mutex::new(());
    let lib = fx.app.library();
    let cancel = meeting::jobs::Cancel::default();
    let r = fx.rt.block_on(meeting::pipeline::summarise(
        &lib, &lock, &fx.id, &plan, &cancel,
    ));
    assert_eq!(
        r.unwrap(),
        meeting::status::Phase::Failed(meeting::status::FailedAt::Summarising)
    );
    let m = fx.meeting();
    let err = m["error"].as_str().unwrap();
    assert!(err.starts_with("Template error on line 2"), "{err}");
    assert_eq!(fx.transcript(), before);
    assert_eq!(
        m["title"], "Deck review",
        "the merge was not written either"
    );
    assert_eq!(m["todos"].as_array().unwrap().len(), 1);
}

#[test]
fn summarise_again_is_not_offered_before_transcription() {
    let fx = fixture(&["openai", "anthropic"]);
    assert_eq!(
        fx.run(Job::Summarise),
        json!({"error": "Summarise again isn't available while recorded"})
    );
    assert_eq!(fx.meeting()["status"], "recorded");
}

#[test]
fn a_cancelled_or_undatable_summary_fails_summarising() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    fx.reset();
    fx.summary(anthropic(SUMMARY).set_delay(Duration::from_secs(20)));
    let (app, id) = (fx.app.clone(), fx.id.clone());
    let h = std::thread::spawn(move || app.run_job_json(&id, Job::Summarise));
    for _ in 0..100 {
        if fx.app.liveness().jobs.contains(&fx.id) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        parse(&fx.app.cancel_json(&fx.id)),
        json!({"cancelled": true})
    );
    let m = parse(&h.join().unwrap())["meeting"].clone();
    assert_eq!(
        (m["failedAt"].as_str(), m["error"].as_str()),
        (Some("summarising"), Some("Cancelled"))
    );
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.started_at = "yesterday".into();
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    let m = fx.run(Job::Summarise)["meeting"].clone();
    assert_eq!(m["error"], "The meeting's start time can't be read");
}

#[test]
fn a_template_chosen_mid_job_is_kept() {
    // The job summarises with the template it started with; a choice made meanwhile is the
    // one the next Summarise again uses (F11-FR6), not overwritten by the finishing job.
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY).set_delay(Duration::from_millis(800)));
    let (app, id) = (fx.app.clone(), fx.id.clone());
    let job = std::thread::spawn(move || parse(&app.run_job_json(&id, Job::Transcribe)));
    // Wait until the summary request is in flight: the job has read its template by then.
    let t0 = std::time::Instant::now();
    while fx.requests("/v1/messages").is_empty() {
        assert!(t0.elapsed() < Duration::from_secs(10), "no summary request");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        parse(&fx.app.set_template_json(&fx.id, "standup")),
        json!({"ok": true})
    );
    let out = job.join().unwrap();
    assert_eq!(out["meeting"]["status"], "done", "{out}");
    assert_eq!(fx.meeting()["templateId"], "standup");
    // The fallback still records General when the chosen template has gone.
    fx.reset();
    fx.summary(anthropic(SUMMARY));
    fx.app.set_template_json(&fx.id, "general");
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.template_id = Some("gone".into());
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    fx.run(Job::Summarise);
    assert_eq!(fx.meeting()["templateId"], "general");
}

#[test]
fn the_fallback_note_is_added_once() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.template_id = Some("gone".into());
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    fx.run(Job::Transcribe);
    let note = "Template gone was not found; used General meeting";
    assert_eq!(fx.meeting()["note"], note);
    // The meeting now names General; set the gone one again and summarise twice more.
    for _ in 0..2 {
        let mut sc = read_sidecar(&fx.dir, &fx.id);
        sc.template_id = Some("gone".into());
        meeting::sidecar::write(&fx.dir, &sc).unwrap();
        fx.run(Job::Summarise);
    }
    assert_eq!(fx.meeting()["note"], note, "not repeated");
}

#[test]
fn a_transcript_that_cant_be_saved_fails_summarising() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    // A directory where the transcript file goes: the atomic write can't replace it.
    std::fs::create_dir_all(fx.dir.join(format!("{}.transcript.md", fx.id)).join("x")).unwrap();
    let m = fx.run(Job::Transcribe)["meeting"].clone();
    assert_eq!(
        (m["status"].as_str(), m["failedAt"].as_str()),
        (Some("failed"), Some("summarising"))
    );
    let err = m["error"].as_str().unwrap();
    assert!(err.starts_with("The transcript can't be saved ("), "{err}");
    assert!(m["title"].is_null(), "the merge was not written either");
}

#[test]
fn a_bluetooth_mic_is_named_in_the_layout() {
    let fx = fixture(&["openai", "anthropic"]);
    let mut sc = read_sidecar(&fx.dir, &fx.id);
    sc.mic = Some(
        serde_json::from_value(json!({"kind": "bluetooth", "name": "Jabra Evolve2"})).unwrap(),
    );
    let tpl = &meeting::templates::builtins()[0];
    let started = chrono::DateTime::parse_from_rfc3339(&sc.started_at).unwrap();
    let ctx = meeting::pipeline::context(&sc, tpl, &started);
    assert_eq!(ctx.mic, "Bluetooth — Jabra Evolve2");
    sc.mic = None; // legacy audio
    assert_eq!(meeting::pipeline::context(&sc, tpl, &started).mic, "");
}

#[test]
fn the_transcript_file_is_written_once_transcription_ends() {
    // S4 / F5-FR11: with no summary, the file has the transcript and an empty summary.
    let fx = fixture(&["openai", "anthropic"]);
    std::fs::remove_file(fx.e.data.path().join("keys/copilot-key-anthropic.txt")).unwrap();
    let m = fx.run(Job::Transcribe)["meeting"].clone();
    assert_eq!(m["status"], "transcribed");
    assert_eq!(m["transcriptFile"], format!("{}.transcript.md", fx.id));
    let md = fx.transcript();
    assert!(md.starts_with("# Untitled meeting\n"), "{md}");
    assert!(
        md.contains("## Transcript\n[0:00:00] text 1\n[0:15:00] text 2\n"),
        "{md}"
    );
    // A summary that fails afterwards keeps it.
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(ResponseTemplate::new(400).set_body_string("bad"));
    let m = fx.run(Job::Transcribe)["meeting"].clone();
    assert_eq!(m["failedAt"], "summarising");
    assert!(fx.transcript().contains("[0:15:00] text 2"));
}

#[test]
fn the_transcript_file_follows_ticks_renames_and_the_template() {
    // S4: the file is re-rendered after every edit that it shows.
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    let todo = fx.meeting()["todos"][0]["id"].as_str().unwrap().to_owned();
    assert!(fx.transcript().contains("- [ ] Send the deck to Priya"));
    assert_eq!(
        parse(&fx.app.set_todo_done_json(&fx.id, &todo, true)),
        json!({"ok": true})
    );
    assert!(
        fx.transcript().contains("- [x] Send the deck to Priya"),
        "{}",
        fx.transcript()
    );
    fx.app.rename_json(&fx.id, "Renamed");
    assert!(
        fx.transcript().starts_with("# Renamed\n"),
        "{}",
        fx.transcript()
    );
    assert_eq!(
        parse(&fx.app.set_template_json(&fx.id, "standup")),
        json!({"ok": true})
    );
    let first = fx.transcript().lines().nth(1).unwrap().to_owned();
    assert!(first.ends_with(" · Stand-up"), "{first}");
    // No stray temporary files are left beside it.
    let tmps = std::fs::read_dir(&fx.dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .count();
    assert_eq!(tmps, 0);
}

#[test]
fn edits_before_transcription_write_no_transcript_file() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.app.rename_json(&fx.id, "Early");
    assert_eq!(fx.transcript(), "");
    assert!(fx.meeting()["transcriptFile"].is_null());
}

#[test]
fn rewriting_the_transcript_reports_what_stops_it() {
    let fx = fixture(&["openai", "anthropic"]);
    fx.summary(anthropic(SUMMARY));
    fx.run(Job::Transcribe);
    let lib = fx.app.library();
    let lock = std::sync::Mutex::new(());
    let rw = |t: &[meeting::templates::Template]| {
        meeting::pipeline::rewrite_transcript(&lib, &lock, &fx.id, t, false)
    };
    let builtins = meeting::templates::builtins();
    assert_eq!(rw(&[]), Err("No template is available".into()));
    let mut bad = builtins.clone();
    bad[0].layout = "{{ 1 // 0 }}".into();
    assert!(
        rw(&bad)
            .unwrap_err()
            .starts_with("Template error on line 1")
    );
    let good = read_sidecar(&fx.dir, &fx.id);
    let mut sc = good.clone();
    sc.started_at = "not a time".into();
    meeting::sidecar::write(&fx.dir, &sc).unwrap();
    assert_eq!(
        rw(&builtins),
        Err("The meeting's start time can't be read".into())
    );
    meeting::sidecar::write(&fx.dir, &good).unwrap();
    let file = fx.dir.join(format!("{}.transcript.md", fx.id));
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir_all(file.join("x")).unwrap();
    assert!(
        rw(&builtins)
            .unwrap_err()
            .starts_with("The transcript can't be saved (")
    );
    assert_eq!(
        meeting::pipeline::rewrite_transcript(
            &lib,
            &lock,
            "meeting-20200101-000000",
            &builtins,
            true
        ),
        Err("meeting not found".into())
    );
}
