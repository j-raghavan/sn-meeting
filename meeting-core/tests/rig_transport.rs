//! F1-FR8 / R11: rig's three providers send through our client (`rig_http`) and can be pointed
//! at a local mock server by base URL. No test reaches a real provider (F8-FR1).
use meeting::providers::{http_client, rig_http};
use rig_core::providers::anthropic::AnthropicConfig;
use rig_core::providers::gemini::GeminiConfig;
use rig_core::providers::openai::OpenAIConfig;
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn transport() -> rig_reqwest::ReqwestClient {
    rig_http(http_client(Duration::from_secs(5), false).unwrap())
}

fn ids(list: rig_core::model::ModelList) -> Vec<String> {
    list.data.into_iter().map(|m| m.id).collect()
}

#[tokio::test]
async fn openai_lists_models_through_our_client() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-fixture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"object":"list","data":[{"id":"gpt-4o-mini","object":"model","owned_by":"x"}]}),
        ))
        .expect(1)
        .mount(&s)
        .await;
    let client = OpenAIConfig::new("sk-fixture")
        .with_base_url(format!("{}/v1", s.uri()))
        .connect(transport());
    assert_eq!(ids(client.list_models().await.unwrap()), ["gpt-4o-mini"]);
}

#[tokio::test]
async fn anthropic_lists_models_through_our_client() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "ak-fixture"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"id":"claude-x","type":"model","display_name":"X","created_at":"2026-01-01T00:00:00Z"}],
                   "has_more":false,"first_id":"claude-x","last_id":"claude-x"}),
        ))
        .expect(1)
        .mount(&s)
        .await;
    let client = AnthropicConfig::new("ak-fixture")
        .with_base_url(s.uri())
        .connect(transport());
    assert_eq!(ids(client.list_models().await.unwrap()), ["claude-x"]);
}

#[tokio::test]
async fn gemini_lists_models_through_our_client() {
    let s = MockServer::start().await;
    // rig's Gemini wire puts the key in the query string; test_connection does not (it sends
    // x-goog-api-key), which is why it makes its own request.
    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .and(query_param("key", "AIza-fixture"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"models":[{"name":"models/gemini-3.8-flash"}]})),
        )
        .expect(1)
        .mount(&s)
        .await;
    let client = GeminiConfig::new("AIza-fixture")
        .with_base_url(s.uri())
        .connect(transport());
    assert_eq!(
        ids(client.list_models().await.unwrap()),
        ["gemini-3.8-flash"]
    );
}
