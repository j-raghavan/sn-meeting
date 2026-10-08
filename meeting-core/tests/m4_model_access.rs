//! D1 / D2 against a mock server: a 403 for a model the account can't use is a model problem
//! (one automatic fallback, a "can't use" message), not a rejected key; and with no model
//! chosen, transcription uses the first known transcription model the account lists.
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
use wiremock::matchers::{body_string_contains, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const GE: &str = "AIza-SECRET-gemini-0003";

const NO_ACCESS: &str = r#"{"error":{"message":"Project `proj_Xy12` does not have access to model `gpt-transcribe`","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
const BAD_KEY: &str = r#"{"error":{"message":"Incorrect API key provided: sk-test-****0001. You can find your API key at https://platform.openai.com/account/api-keys.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;

struct Fx {
    _e: Env,
    rt: Runtime,
    s: MockServer,
    app: App,
    id: String,
    dir: PathBuf,
}

/// A one-segment recorded meeting; OpenAI (or Gemini) transcribes, nothing summarises.
fn fixture(provider: &str, transcribe_model: Option<&str>) -> Fx {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    let (k, m) = match provider {
        "openai" => (OA, "gpt-4o-mini"),
        _ => (GE, "gemini-3.8-flash"),
    };
    let mut body = format!("provider={provider}\nkey={k}\nmodel={m}\n");
    if let Some(t) = transcribe_model {
        body.push_str(&format!("transcribe_model={t}\n"));
    }
    std::fs::write(key_dir.join(format!("copilot-key-{provider}.txt")), body).unwrap();
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
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    let first = PathBuf::from(b["firstSegmentPath"].as_str().unwrap());
    std::fs::write(&first, b"seg1").unwrap();
    app.finish_json(&id, 60_000, "");
    let dir = first.parent().unwrap().to_path_buf();
    Fx {
        _e: e,
        rt,
        s,
        app,
        id,
        dir,
    }
}

/// The multipart `model` field's value, exactly.
fn model_field(m: &str) -> String {
    format!("name=\"model\"\r\n\r\n{m}\r\n")
}

impl Fx {
    fn mount(&self, m: Mock) {
        self.rt.block_on(m.mount(&self.s));
    }
    /// OpenAI answers `model` with `status` and `body`.
    fn openai_answers(&self, model: &str, status: u16, body: &str) {
        self.mount(
            Mock::given(method("POST"))
                .and(path("/v1/audio/transcriptions"))
                .and(body_string_contains(model_field(model)))
                .respond_with(ResponseTemplate::new(status).set_body_string(body)),
        );
    }
    fn openai_lists(&self, ids: &[&str]) {
        let data: Vec<Value> = ids
            .iter()
            .map(|i| json!({"id": i, "object": "model"}))
            .collect();
        self.mount(
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": data}))),
        );
    }
    fn transcribe(&self) -> Value {
        parse(&self.app.run_job_json(&self.id, Job::Transcribe));
        json!(read_sidecar(&self.dir, &self.id))
    }
    fn requests(&self, p: &str) -> Vec<String> {
        let reqs = self.rt.block_on(self.s.received_requests()).unwrap();
        reqs.iter()
            .filter(|r| r.url.path() == p)
            .map(|r| String::from_utf8_lossy(&r.body).into_owned())
            .collect()
    }
}

const OK: &str = r#"{"text":"hello"}"#;

/// Every segment has text: transcription finished (summarising, unmocked, may then fail).
fn transcribed(m: &Value) {
    assert_ne!(m["failedAt"], "transcribing", "{m}");
    assert!(m["segments"][0]["text"].is_string(), "{m}");
}

#[test]
fn a_model_the_account_cant_use_falls_back_to_one_it_lists() {
    let fx = fixture("openai", Some("gpt-transcribe"));
    fx.openai_lists(&["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini"]);
    fx.openai_answers("gpt-transcribe", 403, NO_ACCESS);
    fx.openai_answers("gpt-4o-transcribe", 200, OK);
    let m = fx.transcribe();
    transcribed(&m);
    assert_eq!(m["segments"][0]["text"], "hello");
    assert_eq!(m["providers"]["transcribe"]["model"], "gpt-4o-transcribe");
    assert_eq!(fx.requests("/v1/audio/transcriptions").len(), 2);
}

#[test]
fn with_no_listing_the_fallback_is_the_next_known_model() {
    let fx = fixture("openai", Some("gpt-transcribe"));
    fx.openai_answers("gpt-transcribe", 403, NO_ACCESS);
    fx.openai_answers("gpt-4o-transcribe", 200, OK);
    let m = fx.transcribe();
    transcribed(&m);
    assert_eq!(m["providers"]["transcribe"]["model"], "gpt-4o-transcribe");
}

#[test]
fn a_second_refusal_says_the_account_cant_use_the_model() {
    let fx = fixture("openai", Some("gpt-transcribe"));
    fx.openai_answers("gpt-transcribe", 403, NO_ACCESS);
    fx.openai_answers(
        "gpt-4o-transcribe",
        403,
        &NO_ACCESS.replace("gpt-transcribe", "gpt-4o-transcribe"),
    );
    let m = fx.transcribe();
    assert_eq!(m["status"], "failed", "{m}");
    assert_eq!(
        m["error"],
        "Your OpenAI account can't use gpt-4o-transcribe"
    );
    assert_eq!(
        fx.requests("/v1/audio/transcriptions").len(),
        2,
        "one fallback only"
    );
}

#[test]
fn a_401_is_a_rejected_key_and_is_not_retried_with_another_model() {
    let fx = fixture("openai", Some("gpt-transcribe"));
    fx.openai_answers("gpt-transcribe", 401, BAD_KEY);
    let m = fx.transcribe();
    assert_eq!(m["error"], "OpenAI rejected the key (401)", "{m}");
    assert_eq!(fx.requests("/v1/audio/transcriptions").len(), 1);
    assert!(!m.to_string().contains(OA));
}

#[test]
fn gemini_falls_back_too() {
    let fx = fixture("gemini", Some("gemini-x"));
    let gone = r#"{"error":{"code":404,"message":"models/gemini-x is not found for API version v1beta, or is not supported for generateContent.","status":"NOT_FOUND"}}"#;
    fx.mount(
        Mock::given(method("POST"))
            .and(path_regex("gemini-x:generateContent$"))
            .respond_with(ResponseTemplate::new(404).set_body_string(gone)),
    );
    fx.mount(
        Mock::given(method("POST"))
            .and(path_regex("gemini-3.8-flash:generateContent$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{"content": {"parts": [{"text": "hola"}]}, "finishReason": "STOP"}]}))),
    );
    let m = fx.transcribe();
    transcribed(&m);
    assert_eq!(m["providers"]["transcribe"]["model"], "gemini-3.8-flash");
}
