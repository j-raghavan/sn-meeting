//! F5-FR7 against a mock server: rig's real errors classified (status, Retry-After, transport),
//! one retry for 429/5xx honouring Retry-After, 413 never retried, cancel aborting an in-flight
//! request, and the whole-call budget. No test reaches a real provider (F8-FR1).
use meeting::jobs::{Cancel, with_retry};
use meeting::keyfile::Provider;
use meeting::providers::{CallError, CallFail, classify_call, http_client, rig_http};
use rig_core::completion::CompletionRequest;
use rig_core::message::Message;
use rig_core::providers::anthropic::AnthropicConfig;
use rig_core::providers::gemini::GeminiConfig;
use rig_core::providers::openai::OpenAIConfig;
use std::time::{Duration, Instant};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const OA: &str = "sk-test-SECRET-openai-0001";
const AN: &str = "sk-ant-SECRET-anthropic-0002";
const GE: &str = "AIza-SECRET-gemini-0003";

fn key(p: Provider) -> &'static str {
    match p {
        Provider::OpenAi => OA,
        Provider::Gemini => GE,
        Provider::Anthropic => AN,
    }
}

/// One rig completion to `uri` as `p`, mapped through `classify_call`.
async fn call(p: Provider, uri: &str) -> Result<String, CallError> {
    let http = rig_http(http_client(Duration::from_secs(5), false).unwrap());
    let req = CompletionRequest::new(Message::user("hello")).max_tokens(16);
    let r = match p {
        Provider::OpenAi => {
            let c = OpenAIConfig::new(OA)
                .with_base_url(format!("{uri}/v1"))
                .connect(http);
            c.completion("m").call(req).await
        }
        Provider::Gemini => {
            let c = GeminiConfig::new(GE).with_base_url(uri).connect(http);
            c.completion("m").call(req).await
        }
        Provider::Anthropic => {
            let c = AnthropicConfig::new(AN).with_base_url(uri).connect(http);
            c.completion("m").call(req).await
        }
    };
    r.map(|r| r.text())
        .map_err(|e| classify_call(p, &e, &[key(p)]))
}

/// A server answering every POST with `t`, and the failure one call gets from it.
async fn fail_with(p: Provider, t: ResponseTemplate) -> CallError {
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(t).mount(&s).await;
    let e = call(p, &s.uri()).await.unwrap_err();
    for k in [OA, AN, GE] {
        assert!(!e.detail.contains(k), "key leaked: {e:?}");
    }
    e
}

#[tokio::test]
async fn replies_are_classified_with_status_and_retry_after() {
    for p in Provider::ALL {
        let body = format!(r#"{{"error":{{"message":"bad key {}"}}}}"#, key(p));
        let e = fail_with(p, ResponseTemplate::new(401).set_body_string(body)).await;
        assert_eq!(
            (e.kind, e.status),
            (CallFail::KeyRejected, Some(401)),
            "{p:?}"
        );
        assert!(
            e.detail.ends_with(" rejected the key (401)"),
            "{}",
            e.detail
        );
        let e = fail_with(p, ResponseTemplate::new(413).set_body_string("too big")).await;
        assert_eq!(e.kind, CallFail::TooLarge, "{p:?}");
        let t = ResponseTemplate::new(429).insert_header("retry-after", "12");
        let e = fail_with(p, t).await;
        let want = (CallFail::Retryable, Some(Duration::from_secs(12)));
        assert_eq!((e.kind, e.retry_after), want, "{p:?}");
        let e = fail_with(p, ResponseTemplate::new(429)).await;
        assert_eq!((e.kind, e.retry_after), (CallFail::Retryable, None));
        let e = fail_with(p, ResponseTemplate::new(503).set_body_string("<html>")).await;
        let got = (e.kind, e.status, e.detail.as_str());
        assert_eq!(got, (CallFail::Retryable, Some(503), "HTTP 503"), "{p:?}");
        let echo = format!("upstream said key={} x-api-key: {}", key(p), key(p));
        let e = fail_with(p, ResponseTemplate::new(500).set_body_string(echo)).await;
        assert_eq!(e.kind, CallFail::Retryable);
        let e = fail_with(p, ResponseTemplate::new(200).set_body_string("not json")).await;
        assert_eq!(e.kind, CallFail::BadResponse, "{p:?} {e:?}");
        assert!(
            e.detail.starts_with("Unexpected reply from "),
            "{}",
            e.detail
        );
    }
    let size = r#"{"error":{"code":400,"message":"Request payload size exceeds the limit: 20971520 bytes."}}"#;
    let e = fail_with(
        Provider::Gemini,
        ResponseTemplate::new(400).set_body_string(size),
    )
    .await;
    assert_eq!(e.kind, CallFail::TooLarge);
    let model = r#"{"type":"error","error":{"type":"invalid_request_error","message":"model: claude-x does not support output_config"}}"#;
    let e = fail_with(
        Provider::Anthropic,
        ResponseTemplate::new(400).set_body_string(model),
    )
    .await;
    assert_eq!(e.kind, CallFail::ModelRejected);
}

#[tokio::test]
async fn transport_failures_never_show_the_url_or_the_key() {
    // Nothing listens on port 9; Gemini's URL would carry ?key=.
    for p in Provider::ALL {
        let e = call(p, "http://127.0.0.1:9").await.unwrap_err();
        assert_eq!(e.kind, CallFail::NoConnection, "{p:?}");
        assert_eq!(e.detail, "No connection", "{p:?}");
    }
}

/// `with_retry` around rig calls to `s`, counting what the server received.
async fn retried(s: &MockServer, cancel: &Cancel, budget: Duration) -> Result<String, CallError> {
    let uri = s.uri();
    with_retry(cancel, budget, || call(Provider::Anthropic, &uri)).await
}

fn anthropic_ok(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "m",
        "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 1, "output_tokens": 1}
    }))
}

#[tokio::test]
async fn a_429_waits_for_retry_after_then_succeeds() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .up_to_n_times(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .respond_with(anthropic_ok("ok"))
        .mount(&s)
        .await;
    let t0 = Instant::now();
    let r = retried(&s, &Cancel::default(), Duration::from_secs(5)).await;
    assert_eq!(r.unwrap(), "ok");
    assert!(t0.elapsed() >= Duration::from_secs(1), "waited Retry-After");
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_second_5xx_and_a_413_are_returned() {
    for (code, calls, kind) in [(503, 2, CallFail::Retryable), (413, 1, CallFail::TooLarge)] {
        let s = MockServer::start().await;
        let t = ResponseTemplate::new(code).insert_header("retry-after", "0");
        Mock::given(method("POST")).respond_with(t).mount(&s).await;
        let e = retried(&s, &Cancel::default(), Duration::from_secs(5)).await;
        assert_eq!(e.unwrap_err().kind, kind);
        assert_eq!(s.received_requests().await.unwrap().len(), calls, "{code}");
    }
}

#[tokio::test]
async fn cancel_aborts_the_in_flight_request() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(anthropic_ok("late").set_delay(Duration::from_secs(10)))
        .mount(&s)
        .await;
    let c = Cancel::default();
    let c2 = c.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c2.cancel();
    });
    let t0 = Instant::now();
    let e = retried(&s, &c, Duration::from_secs(30)).await.unwrap_err();
    assert_eq!(
        (e.kind, e.detail.as_str()),
        (CallFail::Cancelled, "Cancelled")
    );
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
}

#[tokio::test]
async fn the_whole_call_budget_times_out() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(anthropic_ok("late").set_delay(Duration::from_secs(10)))
        .mount(&s)
        .await;
    let t0 = Instant::now();
    let e = retried(&s, &Cancel::default(), Duration::from_millis(200)).await;
    let e = e.unwrap_err();
    assert_eq!(
        (e.kind, e.detail.as_str()),
        (CallFail::TimedOut, "Timed out")
    );
    assert!(t0.elapsed() < Duration::from_secs(2));
}
