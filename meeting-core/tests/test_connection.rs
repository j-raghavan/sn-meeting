//! F10-FR5 / F1-FR9: Test connection against a mock server, the success paths.
mod common;

use common::*;
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ok(body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

#[tokio::test]
async fn openai_connects_and_finds_both_models() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {OA}").as_str()))
        .respond_with(ok(
            json!({"data":[{"id":"gpt-4o-mini"},{"id":"gpt-transcribe"}]}),
        ))
        .expect(2)
        .mount(&s)
        .await;
    let c = core_for(&s.uri(), d.path(), 5000);
    let r = call(&c, "summarise", "openai").await;
    assert_eq!(
        (r["ok"].clone(), r["status"].clone()),
        (json!(true), json!("connected"))
    );
    assert_eq!(
        (r["provider"].clone(), r["model"].clone()),
        (json!("openai"), json!("gpt-4o-mini"))
    );
    assert_eq!(r["model_found"], true);
    let ms = r["ms"].as_u64().unwrap();
    assert_eq!(detail(&r), format!("Connected · {ms} ms"));
    let t = call(&c, "transcribe", "openai").await;
    assert_eq!(
        (t["model"].clone(), t["model_found"].clone()),
        (json!("gpt-transcribe"), json!(true))
    );
    assert_keys_not_in_urls(&s).await;
}

#[tokio::test]
async fn a_missing_model_is_a_warning_not_a_failure() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    Mock::given(path("/v1/models"))
        .respond_with(ok(json!({"data":[{"id":"gpt-other"}]})))
        .mount(&s)
        .await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "summarise", "openai").await;
    assert_eq!(
        (r["ok"].clone(), r["model_found"].clone()),
        (json!(true), json!(false))
    );
    assert!(detail(&r).starts_with("Connected · "), "{r}");
    assert!(
        detail(&r).ends_with(" ms · model gpt-4o-mini not found"),
        "{r}"
    );
}

#[tokio::test]
async fn anthropic_follows_the_cursor() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    let base = || {
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", AN))
            .and(header("anthropic-version", "2023-06-01"))
            // UNVERIFIED against the live API: limit=1000 is accepted.
            .and(query_param("limit", "1000"))
    };
    base()
        .and(query_param_is_missing("after_id"))
        .respond_with(ok(
            json!({"data":[{"id":"claude-a"}],"has_more":true,"last_id":"claude-a"}),
        ))
        .expect(1)
        .mount(&s)
        .await;
    base()
        .and(query_param("after_id", "claude-a"))
        .respond_with(ok(
            json!({"data":[{"id":"claude-b"}],"has_more":false,"last_id":"claude-b"}),
        ))
        .expect(1)
        .mount(&s)
        .await;
    let r = call(
        &core_for(&s.uri(), d.path(), 5000),
        "summarise",
        "anthropic",
    )
    .await;
    assert_eq!(
        (r["status"].clone(), r["model_found"].clone()),
        (json!("connected"), json!(true))
    );
    assert_eq!(r["model"], "claude-b");
    assert_keys_not_in_urls(&s).await;
}

#[tokio::test]
async fn a_cursor_that_never_ends_stops_after_ten_pages() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    Mock::given(path("/v1/models"))
        .respond_with(ok(
            json!({"data":[{"id":"claude-x"}],"has_more":true,"last_id":"claude-x"}),
        ))
        .expect(10)
        .mount(&s)
        .await;
    let r = call(
        &core_for(&s.uri(), d.path(), 5000),
        "summarise",
        "anthropic",
    )
    .await;
    assert_eq!(
        (r["ok"].clone(), r["model_found"].clone()),
        (json!(true), json!(false))
    );
}

#[tokio::test]
async fn gemini_pages_strip_the_models_prefix_and_keep_the_key_out_of_the_url() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    let base = || {
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .and(header("x-goog-api-key", GE))
            .and(query_param("pageSize", "1000"))
            .and(query_param_is_missing("key"))
    };
    base()
        .and(query_param_is_missing("pageToken"))
        .respond_with(ok(
            json!({"models":[{"name":"models/gemini-3.8-flash"}],"nextPageToken":"t2"}),
        ))
        .mount(&s)
        .await;
    base()
        .and(query_param("pageToken", "t2"))
        .respond_with(ok(
            json!({"models":[{"name":"models/gemini-z"}],"nextPageToken":""}),
        ))
        .mount(&s)
        .await;
    let c = core_for(&s.uri(), d.path(), 5000);
    let r = call(&c, "summarise", "gemini").await;
    assert_eq!(
        (r["model"].clone(), r["model_found"].clone()),
        (json!("gemini-z"), json!(true))
    );
    let t = call(&c, "transcribe", "gemini").await;
    assert_eq!(
        (t["model"].clone(), t["model_found"].clone()),
        (json!("gemini-3.8-flash"), json!(true))
    );
    assert_eq!(s.received_requests().await.unwrap().len(), 4);
    assert_keys_not_in_urls(&s).await;
}

#[tokio::test]
async fn a_transcribe_model_line_overrides_the_default() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("copilot-key-gpt.txt"),
        format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\ntranscribe_model=whisper-x\n"),
    )
    .unwrap();
    Mock::given(path("/v1/models"))
        .respond_with(ok(json!({"data":[{"id":"whisper-x"}]})))
        .mount(&s)
        .await;
    let r = call(&core_for(&s.uri(), d.path(), 5000), "transcribe", "openai").await;
    assert_eq!(
        (r["model"].clone(), r["model_found"].clone()),
        (json!("whisper-x"), json!(true))
    );
}

/// F10-FR4: Settings' model override is what Test connection checks, but only for the role's
/// effective provider; testing another provider uses that provider's key file.
#[tokio::test]
async fn the_settings_override_is_checked_for_the_effective_provider_only() {
    let s = MockServer::start().await;
    let d = tempfile::tempdir().unwrap();
    write_keys(d.path());
    Mock::given(path("/v1/models"))
        .and(header("authorization", format!("Bearer {OA}").as_str()))
        .respond_with(ok(json!({"data":[{"id":"gpt-override"}]})))
        .mount(&s)
        .await;
    Mock::given(path("/v1/models"))
        .and(header("x-api-key", AN))
        .respond_with(ok(json!({"data":[{"id":"claude-b"}],"has_more":false})))
        .mount(&s)
        .await;
    let data = tempfile::tempdir().unwrap();
    std::fs::write(
        data.path().join("settings.json"),
        r#"{"summarise": {"provider": "openai", "model": "gpt-override"}}"#,
    )
    .unwrap();
    let mut core = core_for(&s.uri(), d.path(), 5000);
    core.app_data_dir = data.path().into();
    let app = std::sync::Arc::new(meeting::app::App::new(core, d.path().into()));
    let run = |role: &'static str, provider: &'static str| {
        let app = app.clone();
        async move {
            let out = tokio::task::spawn_blocking(move || app.test_connection(role, provider))
                .await
                .unwrap();
            assert_no_key(&out);
            serde_json::from_str::<serde_json::Value>(&out).unwrap()
        }
    };
    let r = run("summarise", "openai").await;
    assert_eq!(
        (r["model"].clone(), r["model_found"].clone()),
        (json!("gpt-override"), json!(true))
    );
    let r = run("summarise", "anthropic").await;
    assert_eq!(
        (r["model"].clone(), r["model_found"].clone()),
        (json!("claude-b"), json!(true))
    );
    // Transcribe has no override: the OpenAI default, which this mock doesn't list.
    let r = run("transcribe", "openai").await;
    assert_eq!(
        (r["model"].clone(), r["model_found"].clone()),
        (json!("gpt-transcribe"), json!(false))
    );
    assert_eq!(
        run("bogus", "openai").await,
        json!({"error": "unknown role"})
    );
}
