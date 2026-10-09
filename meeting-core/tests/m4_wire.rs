//! F5-FR1 / F5-FR9: the HTTP rig 0.44.0 really sends for transcription, recorded from a mock
//! server, and replies with fields we don't know. No test reaches a real provider (F8-FR1).
use meeting::keyfile::{self, Provider, ProviderKey};
use meeting::providers::{
    AudioIn, CallFail, Endpoints, GEMINI_TRANSCRIBE_SYSTEM, NO_SPEECH, http_client, rig_http,
    transcribe,
};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const GE: &str = "AIza-SECRET-gemini-0003";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const FILE: &str = "meeting-20261007-211706-01.m4a";

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

fn audio(prompt: &str) -> AudioIn {
    AudioIn {
        bytes: b"\x00\x00\x00\x18ftypM4A fake-aac".to_vec(),
        file_name: FILE.into(),
        prompt: prompt.into(),
        duration_ms: Some(20_000),
    }
}

/// F5-FR13: a segment this long that comes back with no text is a failure to retry.
fn long(prompt: &str) -> AudioIn {
    AudioIn {
        duration_ms: Some(45_000),
        ..audio(prompt)
    }
}

async fn bodies(s: &MockServer) -> Vec<String> {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| String::from_utf8_lossy(&r.body).into_owned())
        .collect()
}

#[tokio::test]
async fn openai_sends_only_model_and_file_and_ignores_unknown_reply_fields() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "  hello there ", "languages": ["en"], "future": {"x": 1},
            "usage": {"type": "tokens", "input_tokens": 5, "output_tokens": 2, "total_tokens": 7}
        })))
        .mount(&s)
        .await;
    let (k, e) = (key(Provider::OpenAi), ep(&s.uri()));
    let t = transcribe(&http(), &e, &k, "gpt-transcribe", audio("")).await;
    assert_eq!(t.unwrap().text, "hello there");
    let t = transcribe(&http(), &e, &k, "gpt-transcribe", audio("Priya, Supernote")).await;
    assert_eq!(t.unwrap().text, "hello there");
    let reqs = s.received_requests().await.unwrap();
    assert_eq!(reqs[0].headers["authorization"], format!("Bearer {OA}"));
    let b = bodies(&s).await;
    assert!(b[0].contains("name=\"model\"") && b[0].contains("gpt-transcribe"));
    assert!(b[0].contains(&format!("name=\"file\"; filename=\"{FILE}\"")));
    for absent in ["prompt", "response_format", "language", "temperature"] {
        assert!(
            !b[0].contains(&format!("name=\"{absent}\"")),
            "{absent} sent"
        );
    }
    assert!(b[1].contains("name=\"prompt\"") && b[1].contains("Priya, Supernote"));
    for r in &reqs {
        assert!(!r.url.as_str().contains(OA));
    }
}

#[tokio::test]
async fn an_empty_reply_for_a_long_segment_is_retryable() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": " \n"})))
        .mount(&s)
        .await;
    let k = key(Provider::OpenAi);
    let e = transcribe(&http(), &ep(&s.uri()), &k, "m", long(""))
        .await
        .unwrap_err();
    assert_eq!(e.kind, CallFail::Retryable);
    assert_eq!(e.detail, "OpenAI returned no text for 45 s of audio");
    let quarter = AudioIn {
        duration_ms: Some(900_000),
        ..audio("")
    };
    let e = transcribe(&http(), &ep(&s.uri()), &k, "m", quarter)
        .await
        .unwrap_err();
    assert_eq!(e.detail, "OpenAI returned no text for 15 min of audio");
    // Gemini: empty, blocked or cut off is retryable for a long segment; fine for a short one.
    for (parts, finish) in [
        (json!([{"text": ""}]), "STOP"),
        (json!([]), "SAFETY"),
        (json!([{"text": "half a sent"}]), "MAX_TOKENS"),
    ] {
        let s = MockServer::start().await;
        let reply = json!({"candidates": [{"content": {"parts": parts, "role": "model"}, "finishReason": finish}]});
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .mount(&s)
            .await;
        let k = key(Provider::Gemini);
        let e = transcribe(&http(), &ep(&s.uri()), &k, "g", long(""))
            .await
            .unwrap_err();
        assert_eq!(e.kind, CallFail::Retryable, "{finish}");
        assert!(e.detail.starts_with("Gemini returned no"), "{}", e.detail);
        assert!(
            transcribe(&http(), &ep(&s.uri()), &k, "g", audio(""))
                .await
                .is_ok()
        );
    }
}

#[tokio::test]
async fn openai_empty_text_is_no_speech() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": " \n"})))
        .mount(&s)
        .await;
    let t = transcribe(
        &http(),
        &ep(&s.uri()),
        &key(Provider::OpenAi),
        "m",
        audio(""),
    )
    .await;
    assert_eq!(t.unwrap().text, NO_SPEECH);
}

fn gemini_reply(parts: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "candidates": [{"content": {"parts": parts, "role": "model"}, "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5},
        "modelVersion": "gemini-3.8-flash", "responseId": "r1", "future": true
    }))
}

#[tokio::test]
async fn gemini_sends_inline_audio_with_our_instruction_and_vocabulary() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .respond_with(gemini_reply(json!([{"text": "hola "}, {"text": "amigos"}])))
        .mount(&s)
        .await;
    let k = key(Provider::Gemini);
    let t = transcribe(
        &http(),
        &ep(&s.uri()),
        &k,
        "gemini-3.8-flash",
        audio("Priya"),
    )
    .await;
    assert_eq!(t.unwrap().text, "hola amigos");
    let t = transcribe(&http(), &ep(&s.uri()), &k, "gemini-3.8-flash", audio("")).await;
    assert_eq!(t.unwrap().text, "hola amigos");
    let reqs = s.received_requests().await.unwrap();
    let b: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    let part = &b["contents"][0]["parts"][0]["inlineData"];
    assert_eq!(part["mimeType"], "audio/m4a");
    assert!(
        part["data"].as_str().unwrap().len() > 10,
        "base64 audio inline"
    );
    let system = b["systemInstruction"]["parts"][0]["text"].as_str().unwrap();
    assert!(system.starts_with(GEMINI_TRANSCRIBE_SYSTEM), "{system}");
    assert!(system.contains("Never translate"));
    assert!(
        system.ends_with("Names and terms that may be spoken: Priya"),
        "{system}"
    );
    let b1: Value = serde_json::from_slice(&reqs[1].body).unwrap();
    let system = b1["systemInstruction"]["parts"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(system, GEMINI_TRANSCRIBE_SYSTEM, "no vocabulary line");
    // rig's Gemini wire carries the key in the query string (as Test connection notes).
    assert!(reqs[0].url.query().unwrap().contains(&format!("key={GE}")));
}

#[tokio::test]
async fn gemini_silence_is_no_speech() {
    // DEVICE-UNVERIFIED: what Gemini really returns for a silent segment. Both an empty text
    // part and a candidate with no parts are read as no speech (EC6).
    for parts in [json!([{"text": ""}]), json!([])] {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(gemini_reply(parts.clone()))
            .mount(&s)
            .await;
        let k = key(Provider::Gemini);
        let t = transcribe(&http(), &ep(&s.uri()), &k, "g", audio("")).await;
        assert_eq!(t.unwrap().text, NO_SPEECH, "{parts}");
    }
}

#[tokio::test]
async fn rigs_own_gemini_transcription_translates_and_drops_the_prompt() {
    // Why `transcribe` does not use rig's Gemini `.transcription()` (M4/M5 rulings).
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(gemini_reply(json!([{"text": "x"}])))
        .mount(&s)
        .await;
    let c = rig_core::providers::gemini::GeminiConfig::new(GE)
        .with_base_url(s.uri())
        .connect(rig_http(http()));
    let req = rig_core::transcription::TranscriptionRequestBuilder::new(b"abc".to_vec())
        .filename(Some("a.m4a".to_string()))
        .prompt("VOCAB-HINT".into())
        .build();
    c.transcription("g").call(req).await.unwrap();
    let body = bodies(&s).await.remove(0);
    assert!(!body.contains("VOCAB-HINT"), "prompt dropped");
    assert!(body.contains("Translate the provided audio"), "{body}");
}

#[tokio::test]
async fn anthropic_cannot_transcribe() {
    let e = transcribe(
        &http(),
        &ep("http://127.0.0.1:9"),
        &key(Provider::Anthropic),
        "m",
        audio(""),
    )
    .await
    .unwrap_err();
    assert_eq!(e.kind, CallFail::Http);
    assert_eq!(e.detail, "Anthropic can't transcribe audio");
}

#[tokio::test]
async fn transcription_failures_are_classified_and_scrubbed() {
    for p in [Provider::OpenAi, Provider::Gemini] {
        for (code, kind) in [
            (401, CallFail::KeyRejected),
            (413, CallFail::TooLarge),
            (429, CallFail::Retryable),
        ] {
            let s = MockServer::start().await;
            let body = format!("echo {}", key(p).expose());
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(code).set_body_string(body))
                .mount(&s)
                .await;
            let e = transcribe(&http(), &ep(&s.uri()), &key(p), "m", audio(""))
                .await
                .unwrap_err();
            assert_eq!(e.kind, kind, "{p:?} {code}");
            assert!(!e.detail.contains(OA) && !e.detail.contains(GE), "{e:?}");
        }
    }
}
