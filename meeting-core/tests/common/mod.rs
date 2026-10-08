//! Shared fixtures for the Test connection suites: fixture keys, a key folder, a `Core` pointed
//! at a wiremock server, and the key-leak assertions (INV2, F4-AC2).
#![allow(dead_code)] // each test binary uses a different subset

use meeting::api::{self, Core};
use meeting::providers::Endpoints;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use wiremock::MockServer;

pub const OA: &str = "sk-test-SECRET-openai-0001";
pub const AN: &str = "sk-ant-SECRET-anthropic-0002";
pub const GE: &str = "AIza-SECRET-gemini-0003";
pub const KEYS: [&str; 3] = [OA, AN, GE];

/// One valid key file per provider, under an alias name for Anthropic.
pub fn write_keys(dir: &Path) {
    let files = [
        (
            "copilot-key-openai.txt",
            format!("provider=openai\nkey={OA}\nmodel=gpt-4o-mini\n"),
        ),
        (
            "copilot-key-claude.txt",
            format!("provider=anthropic\nkey={AN}\nmodel=claude-b\n"),
        ),
        (
            "copilot-key-gemini.txt",
            format!("provider=gemini\nkey={GE}\nmodel=gemini-z\n"),
        ),
    ];
    for (name, body) in files {
        std::fs::write(dir.join(name), body).unwrap();
    }
}

pub fn core_for(uri: &str, key_dir: &Path, timeout_ms: u64) -> Core {
    Core {
        app_data_dir: key_dir.into(),
        key_dir: key_dir.into(),
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.to_owned(),
            anthropic: uri.to_owned(),
        },
        timeout: Duration::from_millis(timeout_ms),
        https_only: false,
    }
}

pub fn assert_no_key(text: &str) {
    for k in KEYS {
        assert!(!text.contains(k), "key leaked: {text}");
    }
}

/// `api::test_connection_model` (no override) off the async runtime (it blocks on the core's own runtime),
/// with the key-leak assertion on the raw JSON.
pub async fn call(core: &Core, role: &'static str, provider: &'static str) -> Value {
    let core = core.clone();
    let out = tokio::task::spawn_blocking(move || {
        api::test_connection_model(&core, role, provider, None)
    })
    .await
    .unwrap();
    assert_no_key(&out);
    serde_json::from_str(&out).unwrap()
}

/// Keys travel in headers only: no request URL the server saw may contain one.
pub async fn assert_keys_not_in_urls(server: &MockServer) {
    for r in server.received_requests().await.unwrap() {
        assert_no_key(r.url.as_str());
    }
}

pub fn detail(v: &Value) -> &str {
    v["detail"].as_str().unwrap()
}
