//! S16: a slow layout render never holds the core write lock, so the recorder's add_segment
//! (and any edit) carries on while a summary renders; and an edit made while a render is out of
//! the lock is never lost from the file. Renders are held open or interleaved with edits through
//! the test hook; this binary has one test, so the hook reaches nothing else.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::{App, Job};
use meeting::providers::Endpoints;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// (render entered, render released)
static GATE: (Mutex<(bool, bool)>, Condvar) = (Mutex::new((false, false)), Condvar::new());

fn hold() {
    let (m, cv) = &GATE;
    let mut g = m.lock().unwrap();
    g.0 = true;
    cv.notify_all();
    let _ = cv
        .wait_timeout_while(g, Duration::from_secs(10), |g| !g.1)
        .unwrap();
}

fn wait_entered() {
    let (m, cv) = &GATE;
    let g = m.lock().unwrap();
    let (g, t) = cv
        .wait_timeout_while(g, Duration::from_secs(10), |g| !g.0)
        .unwrap();
    assert!(!t.timed_out() && g.0, "the summary never rendered");
}

fn release() {
    let (m, cv) = &GATE;
    m.lock().unwrap().1 = true;
    cv.notify_all();
}

const SUMMARY: &str = r#"{"title":"T","summary":"S","decisions":[],"todos":[],"sections":[]}"#;

#[test]
fn add_segment_runs_while_a_summary_renders() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let keys = e.data.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(
        keys.join("copilot-key-openai.txt"),
        "provider=openai\nkey=sk-o\nmodel=gpt-4o-mini\ntranscribe_model=whisper-1\n",
    )
    .unwrap();
    let uri = s.uri();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir: keys,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.clone(),
            anthropic: uri,
        },
        timeout: Duration::from_secs(5),
        https_only: false,
    };
    let app = Arc::new(App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT))));
    let b = begin(&app, T0);
    let a = b["id"].as_str().unwrap().to_owned();
    std::fs::write(
        PathBuf::from(b["firstSegmentPath"].as_str().unwrap()),
        b"seg",
    )
    .unwrap();
    app.finish_json(&a, 60_000, "");
    let mocks = [
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "hello"}))),
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "r", "object": "response", "created_at": 1, "status": "completed", "model": "m",
                "output": [{"type": "message", "id": "m", "status": "completed", "role": "assistant",
                    "content": [{"type": "output_text", "text": SUMMARY, "annotations": []}]}],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}))),
    ];
    for m in mocks {
        rt.block_on(m.mount(&s));
    }
    assert_eq!(
        parse(&app.run_job_json(&a, Job::Transcribe))["meeting"]["status"],
        "done"
    );
    // A second meeting is recording while the first is summarised again.
    let b2 = begin(&app, T0 + 120_000);
    let rec = b2["id"].as_str().unwrap().to_owned();
    let next = PathBuf::from(b2["firstSegmentPath"].as_str().unwrap())
        .with_file_name(format!("{rec}-02.m4a"));
    *meeting::templates::RENDER_HOOK.write().unwrap() = Some(hold);
    let job = {
        let (app, a) = (app.clone(), a.clone());
        std::thread::spawn(move || app.run_job_json(&a, Job::Summarise))
    };
    wait_entered();
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let (app, rec) = (app.clone(), rec.clone());
        std::thread::spawn(move || {
            let _ = tx.send(app.add_segment_json(&rec, next.to_str().unwrap(), 900_000));
        });
    }
    let added = rx.recv_timeout(Duration::from_secs(2));
    release();
    let added = added.expect("add_segment waited for the render");
    assert_eq!(parse(&added)["index"], 1, "{added}");
    *meeting::templates::RENDER_HOOK.write().unwrap() = None;
    assert_eq!(parse(&job.join().unwrap())["meeting"]["status"], "done");
    edits_during_a_render_reach_the_file(&app, &a);
}

static CHURN: OnceLock<(PathBuf, String)> = OnceLock::new();
static CALLS: AtomicUsize = AtomicUsize::new(0);

/// The first three renders each rename the meeting (outside the lock), as a user would mid-render.
fn churn() {
    let n = CALLS.fetch_add(1, Ordering::SeqCst) + 1;
    if n <= 3 {
        let (dir, id) = CHURN.get().unwrap();
        let z = tz(PDT);
        library(&[dir.as_path()], &z)
            .rename(id, &format!("Title {n}"))
            .unwrap();
    }
}

fn edits_during_a_render_reach_the_file(app: &App, id: &str) {
    let dir = app.library().folders[0].clone();
    CHURN.set((dir.clone(), id.to_owned())).unwrap();
    let md = || std::fs::read_to_string(dir.join(format!("{id}.transcript.md"))).unwrap();
    let lock = Mutex::new(());
    *meeting::templates::RENDER_HOOK.write().unwrap() = Some(churn);
    // Summarise again: each render out of the lock sees a newer title, so it renders again;
    // the last try renders under the lock, with the title as it now is.
    let plan = app.plan(Job::Summarise).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cancel = meeting::jobs::Cancel::default();
    let p = rt.block_on(meeting::pipeline::summarise(
        &app.library(),
        &lock,
        id,
        &plan,
        &cancel,
    ));
    assert_eq!(p.unwrap(), meeting::status::Phase::Done);
    assert!(md().starts_with("# Title 3\n"), "{}", md());
    assert_eq!(CALLS.load(Ordering::SeqCst), 4);
    // The same for a re-render after an edit.
    CALLS.store(0, Ordering::SeqCst);
    let templates = app.templates.effective();
    meeting::pipeline::rewrite_transcript(&app.library(), &lock, id, &templates, false).unwrap();
    assert!(md().starts_with("# Title 3\n"), "{}", md());
    assert_eq!(CALLS.load(Ordering::SeqCst), 4);
    *meeting::templates::RENDER_HOOK.write().unwrap() = None;
}
