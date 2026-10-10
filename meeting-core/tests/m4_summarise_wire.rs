//! F5-FR4 / F5-FR6 / F5-FR9: the structured-output request rig 0.44.0 sends to each provider,
//! recorded from a mock server, and how stop reasons and replies are read.
use meeting::keyfile::{self, Provider, ProviderKey};
use meeting::providers::{CallFail, Endpoints, JsonAsk, complete_json, http_client};
use meeting::summarise::{MAX_OUTPUT_TOKENS, response_schema};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const GE: &str = "AIza-SECRET-gemini-0003";
const SUMMARY: &str = r#"{"title":"T","summary":"S","decisions":[],"todos":[{"text":"x","owner":null,"due":null}],"sections":[]}"#;

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

fn ask() -> JsonAsk {
    JsonAsk {
        system: "SYSTEM-INSTRUCTIONS".into(),
        user: "Transcript:\n<transcript>\n[0:00:00] a\n\n[0:15:00] b\n</transcript>".into(),
        schema: response_schema(),
        max_tokens: MAX_OUTPUT_TOKENS,
        images: vec![],
    }
}

fn http() -> rig_reqwest::reqwest::Client {
    http_client(Duration::from_secs(5), false).unwrap()
}

fn openai_reply(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "resp_1", "object": "response", "created_at": 1, "status": "completed",
        "model": "gpt-4o-mini", "future": 1,
        "output": [{"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": []}]}],
        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
    }))
}

fn gemini_reply(text: &str, finish: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "candidates": [{"content": {"parts": [{"text": text}], "role": "model"},
                        "finishReason": finish}],
        "modelVersion": "gemini-3.8-flash", "future": [1]
    }))
}

fn anthropic_reply(text: &str, stop: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [{"type": "thinking", "thinking": "", "signature": "x"},
                    {"type": "text", "text": text}],
        "stop_reason": stop, "stop_sequence": null, "future": 1,
        "usage": {"input_tokens": 1, "output_tokens": 1}
    }))
}

async fn body(s: &MockServer, i: usize) -> Value {
    serde_json::from_slice(&s.received_requests().await.unwrap()[i].body).unwrap()
}

#[tokio::test]
async fn openai_uses_responses_with_a_strict_schema_and_no_store() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(openai_reply(SUMMARY))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let out = complete_json(&http(), &ep(&s.uri()), &k, "gpt-4o-mini", &ask()).await;
    assert_eq!(out.unwrap().text, SUMMARY);
    let b = body(&s, 0).await;
    assert_eq!(b["model"], "gpt-4o-mini");
    assert_eq!(b["text"]["format"]["type"], "json_schema");
    assert_eq!(b["text"]["format"]["strict"], true);
    assert_eq!(b["text"]["format"]["schema"]["additionalProperties"], false);
    assert_eq!(b["instructions"], "SYSTEM-INSTRUCTIONS");
    assert_eq!(b["max_output_tokens"], MAX_OUTPUT_TOKENS);
    for absent in ["store", "temperature", "include"] {
        assert!(b.get(absent).is_none(), "{absent} sent: {b}");
    }
    assert!(
        b.to_string().contains("[0:15:00] b"),
        "the whole transcript in one request"
    );
    let r = &s.received_requests().await.unwrap()[0];
    assert_eq!(r.headers["authorization"], format!("Bearer {OA}"));
}

#[tokio::test]
async fn gemini_uses_a_response_json_schema() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .respond_with(gemini_reply(SUMMARY, "STOP"))
        .mount(&s)
        .await;
    let k = key(Provider::Gemini);
    let out = complete_json(&http(), &ep(&s.uri()), &k, "gemini-3.8-flash", &ask()).await;
    assert_eq!(out.unwrap().text, SUMMARY);
    let b = body(&s, 0).await;
    let g = &b["generationConfig"];
    assert_eq!(g["responseMimeType"], "application/json");
    assert_eq!(
        g["responseJsonSchema"]["required"],
        response_schema()["required"]
    );
    assert_eq!(g["maxOutputTokens"], MAX_OUTPUT_TOKENS);
    assert_eq!(
        b["systemInstruction"]["parts"][0]["text"],
        "SYSTEM-INSTRUCTIONS"
    );
}

#[tokio::test]
async fn anthropic_gets_adaptive_thinking_and_no_sampling_params() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(anthropic_reply(SUMMARY, "end_turn"))
        .mount(&s)
        .await;
    let k = key(Provider::Anthropic);
    let out = complete_json(&http(), &ep(&s.uri()), &k, "claude-opus-5-5", &ask()).await;
    assert_eq!(out.unwrap().text, SUMMARY, "thinking blocks are not text");
    let b = body(&s, 0).await;
    assert_eq!(b["output_config"]["format"]["type"], "json_schema");
    assert_eq!(b["max_tokens"], MAX_OUTPUT_TOKENS);
    assert_eq!(b["thinking"]["type"], "adaptive");
    for banned in ["temperature", "top_p", "top_k"] {
        assert!(b.get(banned).is_none(), "anthropic sent {banned}");
    }
    assert_eq!(b["system"][0]["text"], "SYSTEM-INSTRUCTIONS");
    let r = &s.received_requests().await.unwrap()[0];
    assert_eq!(r.headers["x-api-key"], AN);
    assert_eq!(r.headers["anthropic-version"], "2023-06-01");
}

async fn fails(p: Provider, t: ResponseTemplate) -> meeting::providers::CallError {
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(t).mount(&s).await;
    let e = complete_json(&http(), &ep(&s.uri()), &key(p), "m", &ask())
        .await
        .unwrap_err();
    for k in [OA, AN, GE] {
        assert!(!e.detail.contains(k), "{e:?}");
    }
    e
}

#[tokio::test]
async fn stop_reasons_other_than_a_natural_stop_fail() {
    let e = fails(Provider::Anthropic, anthropic_reply("{\"tit", "max_tokens")).await;
    assert_eq!(
        (e.kind, e.detail.as_str()),
        (CallFail::BadResponse, "The summary was cut off")
    );
    let e = fails(Provider::Anthropic, anthropic_reply("", "refusal")).await;
    assert_eq!(e.detail, "Anthropic declined to summarise this meeting");
    let e = fails(Provider::Gemini, gemini_reply("{", "MAX_TOKENS")).await;
    assert_eq!(e.detail, "The summary was cut off");
    let e = fails(Provider::Gemini, gemini_reply("", "SAFETY")).await;
    assert!(e.detail.starts_with("Gemini stopped: "), "{}", e.detail);
}

#[tokio::test]
async fn an_oversized_reply_is_refused() {
    let big = "x".repeat(meeting::providers::MAX_REPLY_CHARS + 1);
    let e = fails(Provider::OpenAi, openai_reply(&big)).await;
    assert_eq!(e.detail, "The summary was too long");
}

#[tokio::test]
async fn a_schema_that_is_not_an_object_is_refused_before_sending() {
    let mut a = ask();
    a.schema = json!(["not", "an", "object"]);
    let k = key(Provider::OpenAi);
    let e = complete_json(&http(), &ep("http://127.0.0.1:9"), &k, "m", &a).await;
    let e = e.unwrap_err();
    assert_eq!(e.detail, "internal: the schema is not an object");
}
