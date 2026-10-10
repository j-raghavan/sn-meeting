//! F19-AC4 (wire): note region images reach each provider as image parts, not as rig's text
//! placeholder, in one structured request that also carries the `notes` schema field.
use base64::Engine;
use meeting::keyfile::{self, Provider, ProviderKey};
use meeting::providers::{Endpoints, ImageIn, JsonAsk, complete_json, http_client};
use meeting::summarise::{MAX_OUTPUT_TOKENS, response_schema_with_notes};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPLY: &str =
    r#"{"title":"T","summary":"S","decisions":[],"todos":[],"sections":[],"notes":[]}"#;
/// Two distinct "PNGs": the signature and a marker; rig never decodes them.
fn png(n: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    b.extend([n; 32]);
    b
}

fn key(p: Provider) -> ProviderKey {
    let (id, k) = match p {
        Provider::OpenAi => ("openai", "sk-test-openai"),
        Provider::Gemini => ("gemini", "AIza-test-gemini"),
        Provider::Anthropic => ("anthropic", "sk-ant-test"),
    };
    keyfile::parse(&format!("provider={id}\nkey={k}\nmodel=m\n"), p).unwrap()
}

fn ask() -> JsonAsk {
    JsonAsk {
        system: "S".into(),
        user: "Transcript:\n<transcript>\n[0:00:00] a\n</transcript>".into(),
        schema: response_schema_with_notes(),
        max_tokens: MAX_OUTPUT_TOKENS,
        images: vec![
            ImageIn {
                label: "Note region 0:".into(),
                png: png(1),
            },
            ImageIn {
                label: "Note region 1:".into(),
                png: png(2),
            },
        ],
    }
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// Every object in `v` (depth first).
fn objects(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Object(o) => {
            out.push(v.clone());
            o.values().for_each(|x| objects(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| objects(x, out)),
        _ => {}
    }
}

async fn run(p: Provider, route: &str, reply: Value, model: &str) -> Value {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&s)
        .await;
    let uri = s.uri();
    let ep = Endpoints {
        openai: format!("{uri}/v1"),
        gemini: uri.clone(),
        anthropic: uri,
    };
    let http = http_client(Duration::from_secs(5), false).unwrap();
    let r = complete_json(&http, &ep, &key(p), model, &ask()).await;
    assert_eq!(r.unwrap().text, REPLY);
    let reqs = s.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1, "exactly one request");
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let text = body.to_string();
    assert!(text.contains("Note region 1:"), "labels sent: {text}");
    assert!(
        !text.to_lowercase().contains("[image"),
        "no placeholder: {text}"
    );
    body
}

#[tokio::test]
async fn openai_sends_input_images() {
    let reply = json!({"id": "r", "object": "response", "created_at": 1, "status": "completed",
        "model": "gpt-4o-mini", "output": [{"type": "message", "id": "m", "status": "completed",
        "role": "assistant", "content": [{"type": "output_text", "text": REPLY, "annotations": []}]}],
        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}});
    let b = run(Provider::OpenAi, "/v1/responses", reply, "gpt-4o-mini").await;
    let mut all = Vec::new();
    objects(&b, &mut all);
    let imgs: Vec<&Value> = all.iter().filter(|o| o["type"] == "input_image").collect();
    assert_eq!(imgs.len(), 2, "{b}");
    for (i, n) in [(0, 1), (1, 2)] {
        assert_eq!(
            imgs[i]["image_url"],
            format!("data:image/png;base64,{}", b64(&png(n)))
        );
        assert_eq!(imgs[i]["detail"], "high");
    }
    let schema = &b["text"]["format"]["schema"];
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("notes"))
    );
    assert_eq!(schema["properties"]["notes"]["type"], "array");
}

#[tokio::test]
async fn gemini_sends_inline_data() {
    let reply = json!({"candidates": [{"content": {"parts": [{"text": REPLY}], "role": "model"},
        "finishReason": "STOP"}], "modelVersion": "gemini-3.8-flash"});
    let route = "/v1beta/models/gemini-3.8-flash:generateContent";
    let b = run(Provider::Gemini, route, reply, "gemini-3.8-flash").await;
    let mut all = Vec::new();
    objects(&b, &mut all);
    let imgs: Vec<&Value> = all
        .iter()
        .filter(|o| o.get("inlineData").is_some())
        .collect();
    assert_eq!(imgs.len(), 2, "{b}");
    for (i, n) in [(0, 1), (1, 2)] {
        assert_eq!(imgs[i]["inlineData"]["mimeType"], "image/png");
        assert_eq!(imgs[i]["inlineData"]["data"], b64(&png(n)));
    }
    let schema = &b["generationConfig"]["responseJsonSchema"];
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("notes"))
    );
}

#[tokio::test]
async fn anthropic_sends_base64_images() {
    let reply = json!({"id": "msg", "type": "message", "role": "assistant",
        "model": "claude-opus-5-5", "content": [{"type": "text", "text": REPLY}],
        "stop_reason": "end_turn", "usage": {"input_tokens": 1, "output_tokens": 1}});
    let b = run(
        Provider::Anthropic,
        "/v1/messages",
        reply,
        "claude-opus-5-5",
    )
    .await;
    let mut all = Vec::new();
    objects(&b, &mut all);
    let imgs: Vec<&Value> = all.iter().filter(|o| o["type"] == "image").collect();
    assert_eq!(imgs.len(), 2, "{b}");
    for (i, n) in [(0, 1), (1, 2)] {
        assert_eq!(imgs[i]["source"]["type"], "base64");
        assert_eq!(imgs[i]["source"]["media_type"], "image/png");
        assert_eq!(imgs[i]["source"]["data"], b64(&png(n)));
    }
}
