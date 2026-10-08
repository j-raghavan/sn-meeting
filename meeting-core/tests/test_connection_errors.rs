//! F10-FR5: Test connection's failure paths. Every reply is checked for the fixture keys.
mod common;

use common::*;
use serde_json::json;
use std::time::Duration;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn replying(t: ResponseTemplate) -> (MockServer, tempfile::TempDir) {
    let s = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(t)
        .mount(&s)
        .await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    (s, d)
}

#[tokio::test]
async fn a_rejected_key_is_reported_without_the_echoed_key() {
    let (s, d) =
        replying(ResponseTemplate::new(401).set_body_string(format!("bad key {OA}"))).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(
        (r["ok"].clone(), r["status"].clone()),
        (json!(false), json!("key_rejected"))
    );
    assert_eq!(detail(&r), "Key rejected (401)");
    assert_eq!(r["model_found"], json!(null));
    let (s, d) = replying(ResponseTemplate::new(403)).await;
    let r = call(
        &core_for(&s.uri(), d.path(), 5000),
        "summarise",
        "anthropic",
    )
    .await;
    assert_eq!(detail(&r), "Key rejected (403)");
}

#[tokio::test]
async fn gemini_api_key_invalid_is_a_rejected_key() {
    // UNVERIFIED against the live API: Gemini answers a bad key with 400 + API_KEY_INVALID.
    let body = json!({"error":{"code":400,"message":"API key not valid.","status":"INVALID_ARGUMENT",
        "details":[{"reason":"API_KEY_INVALID"}]}});
    let (s, d) = replying(ResponseTemplate::new(400).set_body_json(body)).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "gemini").await;
    assert_eq!(
        (r["status"].clone(), detail(&r)),
        (json!("key_rejected"), "Key rejected (400)")
    );
}

#[tokio::test]
async fn a_server_error_echoing_the_key_is_scrubbed() {
    let body = json!({"error":{"message":format!("boom {AN} x-api-key: {AN}")}});
    let (s, d) = replying(ResponseTemplate::new(500).set_body_json(body)).await;
    let r = call(
        &core_for(&s.uri(), d.path(), 5000),
        "summarise",
        "anthropic",
    )
    .await;
    assert_eq!(r["status"], "http_error");
    assert_eq!(
        detail(&r),
        "HTTP 500: boom [redacted] x-api-key: [redacted]"
    );
}

#[tokio::test]
async fn plain_text_and_html_error_bodies() {
    let text = format!("upstream failed\nAuthorization: Bearer {OA}");
    let (s, d) = replying(ResponseTemplate::new(503).set_body_string(text)).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(detail(&r), "HTTP 503: upstream failed");
    let (s, d) =
        replying(ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>")).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(detail(&r), "HTTP 502");
    let (s, d) = replying(ResponseTemplate::new(429)).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(detail(&r), "HTTP 429");
}

#[tokio::test]
async fn an_unreadable_success_reply_is_an_http_error() {
    let (s, d) = replying(ResponseTemplate::new(200).set_body_string("not json")).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "gemini").await;
    assert_eq!(
        (r["status"].clone(), detail(&r)),
        (json!("http_error"), "HTTP 200: unexpected response")
    );
    let (s, d) = replying(ResponseTemplate::new(200).set_body_json(json!({"data": 1}))).await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(detail(&r), "HTTP 200: unexpected response");
}

#[tokio::test]
async fn a_slow_reply_times_out() {
    let (s, d) = replying(ResponseTemplate::new(200).set_delay(Duration::from_millis(1500))).await;
    let r = call(&core_for(&s.uri(), d.path(), 300), "summarise", "gemini").await;
    assert_eq!(
        (r["status"].clone(), detail(&r)),
        (json!("timed_out"), "Timed out")
    );
    assert!(r["ms"].as_u64().unwrap() < 1500, "{r}");
}

#[tokio::test]
async fn nothing_listening_is_no_connection() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    let r = call(
        &core_for(&format!("http://127.0.0.1:{port}"), d.path(), 5000),
        "summarise",
        "openai",
    )
    .await;
    assert_eq!(
        (r["status"].clone(), detail(&r)),
        (json!("no_connection"), "No connection")
    );
}

#[tokio::test]
async fn plain_http_is_refused_on_the_device_core() {
    let (s, d) = replying(ResponseTemplate::new(200)).await;
    let mut c = core_for(&s.uri(), d.path(), 5000);
    c.https_only = true;
    let r = call(&c, "summarise", "openai").await;
    assert_eq!(r["status"], "no_connection");
    assert!(detail(&r).starts_with("No connection: "), "{r}");
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_malformed_endpoint_is_no_connection() {
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    let c = core_for("not a url", d.path(), 5000);
    let r = call(&c, "summarise", "anthropic").await;
    assert_eq!(detail(&r), "No connection: bad endpoint");
}

#[tokio::test]
async fn missing_vaulted_and_broken_key_files() {
    let d = tempfile::tempdir().unwrap();
    let c = core_for("http://127.0.0.1:9", d.path(), 5000);
    let r = call(&c, "summarise", "openai").await;
    assert_eq!(
        (r["ok"].clone(), r["status"].clone()),
        (json!(false), json!("no_key"))
    );
    assert_eq!(
        detail(&r),
        "Add copilot-key-openai.txt to MyStyle/SnCopilot"
    );
    assert_eq!(
        (r["model"].clone(), r["ms"].clone()),
        (json!(null), json!(0))
    );
    std::fs::write(d.path().join(".copilot-key.enc"), "x").unwrap();
    let r = call(&c, "transcribe", "gemini").await;
    assert_eq!(
        detail(&r),
        "Your keys are in SN-Copilot's encrypted vault; Meetings v1 needs the plaintext copilot-key-gemini.txt"
    );
    std::fs::write(
        d.path().join("copilot-key-openai.txt"),
        format!("provider=openai\nkey={OA}\n"),
    )
    .unwrap();
    let r = call(&c, "summarise", "openai").await;
    assert_eq!(r["status"], "key_file_error");
    assert_eq!(detail(&r), "openai key file: missing required field: model");
}

#[tokio::test]
async fn bad_roles_and_providers_are_errors() {
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    let c = core_for("http://127.0.0.1:9", d.path(), 5000);
    for (role, provider, msg) in [
        ("transcribe", "anthropic", "anthropic cannot transcribe"),
        ("summarize", "openai", "unknown role"),
        ("", "openai", "unknown role"),
        ("summarise", "deepseek", "unknown provider"),
        ("summarise", "OpenAI", "unknown provider"),
    ] {
        assert_eq!(
            call(&c, role, provider).await,
            json!({"error": msg}),
            "{role}/{provider}"
        );
    }
}
