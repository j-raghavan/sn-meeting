//! F16-FR1 / F16-AC1..AC4: the usage each provider reports, read from mock-server replies in the
//! shapes their API references document. No test reaches a real provider (F8-FR1).
use meeting::keyfile::{self, Provider, ProviderKey};
use meeting::providers::{
    AudioIn, CallFail, Endpoints, JsonAsk, Measured, complete_json, http_client, transcribe,
};
use meeting::summarise::{MAX_OUTPUT_TOKENS, response_schema};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const GE: &str = "AIza-SECRET-gemini-0003";
const SUMMARY: &str = r#"{"title":"T","summary":"S","decisions":[],"todos":[],"sections":[]}"#;

fn key(p: Provider) -> ProviderKey {
    let (id, k) = match p {
        Provider::OpenAi => ("openai", OA),
        Provider::Gemini => ("gemini", GE),
        Provider::Anthropic => ("anthropic", AN),
    };
    keyfile::parse(&format!("provider={id}\nkey={k}\nmodel=m-{id}\n"), p).unwrap()
}

fn ep(uri: &str) -> Endpoints {
    Endpoints {
        openai: format!("{uri}/v1"),
        gemini: uri.into(),
        anthropic: uri.into(),
    }
}

fn http() -> rig_reqwest::reqwest::Client {
    http_client(Duration::from_secs(5), false).unwrap()
}

fn audio(ms: u64) -> AudioIn {
    AudioIn {
        bytes: b"\x00\x00\x00\x18ftypM4A fake-aac".to_vec(),
        file_name: "meeting-20261009-100000-01.m4a".into(),
        prompt: String::new(),
        duration_ms: Some(ms),
        empty_ok: false,
    }
}

fn ask() -> JsonAsk {
    JsonAsk {
        system: "S".into(),
        user: "U".into(),
        schema: response_schema(),
        max_tokens: MAX_OUTPUT_TOKENS,
        images: vec![],
    }
}

async fn openai_transcription(body: Value, model: &str, ms: u64) -> Measured {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let r = transcribe(&http(), &ep(&s.uri()), &k, model, audio(ms)).await;
    r.unwrap().measured
}

/// F16-AC1 (the measured half): whisper-1 bills by duration, and the seconds come from `raw`.
#[tokio::test]
async fn openai_duration_usage_keeps_the_seconds() {
    let body = json!({"text": "hi", "usage": {"type": "duration", "seconds": 30}});
    let m = openai_transcription(body, "whisper-1", 29_480).await;
    assert_eq!(m.provider, Provider::OpenAi);
    assert_eq!(m.model, "whisper-1");
    assert_eq!(m.usage_type.as_deref(), Some("duration"));
    assert_eq!(m.seconds, Some(30.0));
    assert_eq!(m.audio_ms, Some(29_480));
    assert!(!m.has_tokens());
}

/// F16-AC2 (the measured half): token billing with the audio/text split.
#[tokio::test]
async fn openai_token_usage_keeps_the_audio_and_text_split() {
    let body = json!({"text": "hi", "usage": {"type": "tokens", "input_tokens": 500,
        "input_token_details": {"audio_tokens": 480, "text_tokens": 20},
        "output_tokens": 90, "total_tokens": 590}});
    let m = openai_transcription(body, "gpt-4o-transcribe", 30_000).await;
    assert_eq!(m.usage_type.as_deref(), Some("tokens"));
    assert_eq!((m.input_tokens, m.output_tokens), (Some(500), Some(90)));
    assert_eq!(
        (m.audio_input_tokens, m.text_input_tokens),
        (Some(480), Some(20))
    );
    assert_eq!(m.seconds, None);
}

#[tokio::test]
async fn a_reply_with_no_usage_measures_only_the_audio() {
    let m = openai_transcription(json!({"text": "hi"}), "whisper-1", 1_000).await;
    let want = Measured {
        audio_ms: Some(1_000),
        ..Measured::new(Provider::OpenAi, "whisper-1")
    };
    assert_eq!(m, want);
}

fn gemini_body(text: &str, finish: &str) -> Value {
    json!({
        "candidates": [{"content": {"parts": [{"text": text}], "role": "model"},
                        "finishReason": finish}],
        "usageMetadata": {"promptTokenCount": 1040, "candidatesTokenCount": 40,
            "thoughtsTokenCount": 5, "totalTokenCount": 1085, "cachedContentTokenCount": 0,
            "promptTokensDetails": [{"modality": "AUDIO", "tokenCount": 960},
                                    {"modality": "TEXT", "tokenCount": 80}]},
        "modelVersion": "gemini-3.8-flash-001", "responseId": "r1"
    })
}

/// F16-AC3: both modality counts are kept, so the instruction overhead (80 / 960) shows.
#[tokio::test]
async fn gemini_transcription_keeps_each_modality() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gemini_body("hola", "STOP")))
        .mount(&s)
        .await;
    let k = key(Provider::Gemini);
    let r = transcribe(
        &http(),
        &ep(&s.uri()),
        &k,
        "gemini-3.8-flash",
        audio(30_000),
    )
    .await;
    let m = r.unwrap().measured;
    assert_eq!(m.model, "gemini-3.8-flash-001", "the reply's model");
    assert_eq!(m.input_tokens, Some(1040));
    assert_eq!(
        (m.audio_input_tokens, m.text_input_tokens),
        (Some(960), Some(80))
    );
    assert_eq!(m.output_tokens, Some(45), "thoughts are output");
    assert_eq!(m.audio_ms, Some(30_000));
}

/// A blocked reply for a long segment fails, but its usage was reported and billed.
#[tokio::test]
async fn a_blocked_long_segment_still_reports_what_it_cost() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gemini_body("par", "MAX_TOKENS")))
        .mount(&s)
        .await;
    let k = key(Provider::Gemini);
    let e = transcribe(
        &http(),
        &ep(&s.uri()),
        &k,
        "gemini-3.8-flash",
        audio(45_000),
    )
    .await
    .unwrap_err();
    assert_eq!(e.kind, CallFail::Retryable);
    let spent = e.spent.expect("usage of a billed reply");
    assert_eq!(
        (spent.input_tokens, spent.audio_ms),
        (Some(1040), Some(45_000))
    );
}

#[tokio::test]
async fn an_empty_long_openai_reply_still_reports_what_it_cost() {
    let s = MockServer::start().await;
    let body = json!({"text": " ", "usage": {"type": "duration", "seconds": 45}});
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let e = transcribe(&http(), &ep(&s.uri()), &k, "whisper-1", audio(45_000))
        .await
        .unwrap_err();
    assert_eq!(e.spent.unwrap().seconds, Some(45.0));
}

/// An HTTP failure reports no usage, so nothing is recorded (OQ16).
#[tokio::test]
async fn a_failed_request_reports_no_usage() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let e = transcribe(&http(), &ep(&s.uri()), &k, "whisper-1", audio(1_000))
        .await
        .unwrap_err();
    assert!(e.spent.is_none());
}

async fn summarise_with(p: Provider, route: &str, body: Value, model: &str) -> Measured {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&s)
        .await;
    let r = complete_json(&http(), &ep(&s.uri()), &key(p), model, &ask()).await;
    r.unwrap().measured
}

/// F16-AC4: each provider's summarise usage, counters as in the fixture.
#[tokio::test]
async fn openai_responses_usage() {
    let body = json!({
        "id": "resp_1", "object": "response", "created_at": 1, "status": "completed",
        "model": "gpt-4o-mini-2024-07-18",
        "output": [{"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
            "content": [{"type": "output_text", "text": SUMMARY, "annotations": []}]}],
        "usage": {"input_tokens": 1200, "input_tokens_details": {"cached_tokens": 200},
            "output_tokens": 300, "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": 1500}
    });
    let m = summarise_with(Provider::OpenAi, "/v1/responses", body, "gpt-4o-mini").await;
    assert_eq!(m.model, "gpt-4o-mini-2024-07-18");
    assert_eq!((m.input_tokens, m.output_tokens), (Some(1200), Some(300)));
    assert_eq!(m.cached_input_tokens, Some(200));
    assert_eq!(m.audio_ms, None);
}

#[tokio::test]
async fn gemini_summarise_usage() {
    let path = "/v1beta/models/gemini-3.8-flash:generateContent";
    let mut body = gemini_body(SUMMARY, "STOP");
    body["usageMetadata"]["cachedContentTokenCount"] = json!(100);
    let m = summarise_with(Provider::Gemini, path, body, "gemini-3.8-flash").await;
    assert_eq!((m.input_tokens, m.output_tokens), (Some(1040), Some(45)));
    assert_eq!(m.cached_input_tokens, Some(100));
}

#[tokio::test]
async fn anthropic_input_is_the_sum_of_its_three_fields() {
    let body = json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [{"type": "text", "text": SUMMARY}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": {"input_tokens": 1000, "cache_read_input_tokens": 300,
            "cache_creation_input_tokens": 200, "output_tokens": 400}
    });
    let m = summarise_with(Provider::Anthropic, "/v1/messages", body, "claude-opus-5-5").await;
    assert_eq!(m.input_tokens, Some(1500));
    assert_eq!(m.cached_input_tokens, Some(300));
    assert_eq!(m.cache_write_tokens, Some(200));
    assert_eq!(m.output_tokens, Some(400));
}

/// A cut-off summary is a failure, but the tokens were billed.
#[tokio::test]
async fn a_cut_off_summary_still_reports_what_it_cost() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gemini_body("{", "MAX_TOKENS")))
        .mount(&s)
        .await;
    let k = key(Provider::Gemini);
    let e = complete_json(&http(), &ep(&s.uri()), &k, "gemini-3.8-flash", &ask())
        .await
        .unwrap_err();
    assert_eq!(e.kind, CallFail::BadResponse);
    assert_eq!(e.spent.unwrap().output_tokens, Some(45));
}

/// INV2: a reply that echoes the key in its model name never carries it into a record.
#[tokio::test]
async fn a_model_name_holding_the_key_is_scrubbed() {
    let body = json!({"text": "hi"});
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let m = transcribe(&http(), &ep(&s.uri()), &k, "whisper-1", audio(1_000))
        .await
        .unwrap()
        .measured;
    assert_eq!(m.model, "whisper-1");
    let mut gem = gemini_body("x", "STOP");
    gem["modelVersion"] = json!(format!("m-{GE}"));
    let path = "/v1beta/models/gemini-3.8-flash:generateContent";
    let m = summarise_with(Provider::Gemini, path, gem, "gemini-3.8-flash").await;
    assert!(!m.model.contains(GE), "{}", m.model);
}
