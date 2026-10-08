//! F5: transcription and structured summarising through rig, and what a failure means for the
//! job (F5-FR7). Part of the providers module: no rig or reqwest type leaves it (R8). Every
//! detail is scrubbed with the key and never formats a transport error with its URL (rig's
//! Gemini wire puts `?key=` in the URL).
use super::{ConnStatus, Endpoints, classify_http, classify_transport, rig_http};
use crate::keyfile::{Provider, ProviderKey};
use crate::scrub::scrub;
use rig_core::completion::{CompletionRequest, FinishReason};
use rig_core::message::{AudioMediaType, Message, StopReason, UserContent};
use rig_core::providers::anthropic::AnthropicConfig;
use rig_core::providers::gemini::GeminiConfig;
use rig_core::providers::openai::OpenAIConfig;
use rig_core::transcription::TranscriptionRequestBuilder;
use rig_core::{ProviderError, http_client};
use rig_reqwest::reqwest;
use serde::Serialize;
use std::time::Duration;

/// The HTTP client type, named so job code never imports reqwest (R8).
pub type Http = reqwest::Client;

/// What a failed call means for the job (F5-FR7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallFail {
    /// 401, a 403 naming the key or authentication, or Gemini's 400 `API_KEY_INVALID`.
    KeyRejected,
    /// 413, or Gemini's request-size 400: never retried.
    TooLarge,
    /// 408/425/429/5xx: one automatic retry.
    Retryable,
    /// A 403/404 about the model (the account can't use it), or a 400/404 naming the model or
    /// the output format: a job retries once with another model (F4-FR3, D1).
    ModelRejected,
    /// Any other non-2xx reply.
    Http,
    NoConnection,
    TimedOut,
    /// A 2xx reply that can't be used (not our JSON, no text, cut off).
    BadResponse,
    Cancelled,
}

/// A failed call. `detail` is user-worded, scrubbed and at most `scrub::MAX_LEN` chars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallError {
    pub kind: CallFail,
    pub status: Option<u16>,
    /// `Retry-After` in whole seconds, when the reply carried one.
    pub retry_after: Option<Duration>,
    pub detail: String,
}

impl CallError {
    pub fn new(kind: CallFail, detail: impl Into<String>) -> CallError {
        CallError {
            kind,
            status: None,
            retry_after: None,
            detail: detail.into(),
        }
    }
}

/// One audio segment to transcribe, read from disk by the caller.
pub struct AudioIn {
    pub bytes: Vec<u8>,
    /// The segment's file name; OpenAI infers the format from its extension.
    pub file_name: String,
    /// The template's vocabulary as a hint (F5-FR1). Empty sends model + file only.
    pub prompt: String,
}

/// What a segment with no speech is stored as (EC6).
pub const NO_SPEECH: &str = "(no speech detected)";

/// Gemini audio goes through `generateContent` with this instruction, not rig's
/// `.transcription()`, whose fixed prompt asks to translate and which drops the vocabulary.
pub const GEMINI_TRANSCRIBE_SYSTEM: &str = "Transcribe the speech in this audio verbatim, in the \
language it is spoken. Never translate it. Do not summarise, correct or add anything. Output \
only the transcript as plain text. If there is no speech, output nothing.";

/// F5-FR1: one segment's text. OpenAI: `POST {openai}/audio/transcriptions`, multipart
/// `model` + `file` (+ `prompt` when there is vocabulary). Gemini: `generateContent` with the
/// audio inline (base64) and `GEMINI_TRANSCRIBE_SYSTEM`. Empty text is `NO_SPEECH`.
pub async fn transcribe(
    http: &Http,
    ep: &Endpoints,
    key: &ProviderKey,
    model: &str,
    audio: AudioIn,
) -> Result<String, CallError> {
    let reply = match key.provider {
        Provider::OpenAi => {
            let client = OpenAIConfig::new(key.expose())
                .with_base_url(&ep.openai)
                .connect(rig_http(http.clone()));
            let mut b = TranscriptionRequestBuilder::new(audio.bytes).filename(audio.file_name);
            if !audio.prompt.trim().is_empty() {
                b = b.prompt(audio.prompt);
            }
            client
                .transcription(model)
                .call(b.build())
                .await
                .map(|r| r.text)
        }
        Provider::Gemini => {
            let client = GeminiConfig::new(key.expose())
                .with_base_url(&ep.gemini)
                .connect(rig_http(http.clone()));
            let mut system = GEMINI_TRANSCRIBE_SYSTEM.to_owned();
            if !audio.prompt.trim().is_empty() {
                system.push_str("\nNames and terms that may be spoken: ");
                system.push_str(&audio.prompt);
            }
            let msg = Message::User {
                content: vec![UserContent::audio_raw(
                    audio.bytes,
                    Some(AudioMediaType::M4A),
                )],
            };
            let req = CompletionRequest::new(msg).preamble(system);
            client.completion(model).call(req).await.map(|r| r.text())
        }
        Provider::Anthropic => {
            return Err(CallError::new(
                CallFail::Http,
                "Anthropic can't transcribe audio",
            ));
        }
    };
    let text = reply.map_err(|e| classify_call(key.provider, model, &e, &[key.expose()]))?;
    let text = text.trim();
    Ok(if text.is_empty() { NO_SPEECH } else { text }.to_owned())
}

/// The longest reply text kept; a structured summary of a long meeting is ~10 KB.
pub const MAX_REPLY_CHARS: usize = 256 * 1024;

/// One structured-output request (F5-FR4).
pub struct JsonAsk {
    pub system: String,
    pub user: String,
    pub schema: serde_json::Value,
    pub max_tokens: u64,
}

/// F5-FR4: one structured completion; returns the model's JSON text, not yet validated.
/// OpenAI goes through the Responses API (`text.format` json_schema strict; `store` is not
/// sent). Anthropic gets rig's defaults for the model: adaptive thinking, no sampling params.
pub async fn complete_json(
    http: &Http,
    ep: &Endpoints,
    key: &ProviderKey,
    model: &str,
    ask: &JsonAsk,
) -> Result<String, CallError> {
    let secret = [key.expose()];
    let name = provider_name(key.provider);
    let bad = |d: String| CallError::new(CallFail::BadResponse, scrub(&d, &secret));
    let schema = rig_core::schemars::Schema::try_from(ask.schema.clone())
        .map_err(|_| bad("internal: the schema is not an object".into()))?;
    let req = CompletionRequest::new(Message::user(ask.user.clone()))
        .preamble(ask.system.clone())
        .max_tokens(ask.max_tokens)
        .output_schema(schema);
    let t = rig_http(http.clone());
    let reply = match key.provider {
        Provider::OpenAi => {
            let c = OpenAIConfig::new(key.expose())
                .with_base_url(&ep.openai)
                .connect(t);
            c.completion(model).call(req).await
        }
        Provider::Gemini => {
            let c = GeminiConfig::new(key.expose())
                .with_base_url(&ep.gemini)
                .connect(t);
            c.completion(model).call(req).await
        }
        Provider::Anthropic => {
            let c = AnthropicConfig::new(key.expose())
                .with_base_url(&ep.anthropic)
                .connect(t);
            c.completion(model).call(req).await
        }
    };
    let r = reply.map_err(|e| classify_call(key.provider, model, &e, &secret))?;
    match r.stop() {
        StopReason::Stop => {}
        StopReason::Length => return Err(bad("The summary was cut off".into())),
        StopReason::Error(_)
            if key.provider == Provider::Anthropic
                && r.finish_reason() == Some(FinishReason::ContentFilter) =>
        {
            return Err(bad("Anthropic declined to summarise this meeting".into()));
        }
        other => return Err(bad(format!("{name} stopped: {}", stop_text(&other)))),
    }
    let text = r.text();
    if text.chars().count() > MAX_REPLY_CHARS {
        return Err(bad("The summary was too long".into()));
    }
    Ok(text)
}

fn stop_text(s: &StopReason) -> String {
    match s {
        StopReason::Error(m) | StopReason::Aborted(m) => m.clone(),
        other => format!("{other:?}"),
    }
}

pub fn provider_name(p: Provider) -> &'static str {
    match p {
        Provider::OpenAi => "OpenAI",
        Provider::Gemini => "Gemini",
        Provider::Anthropic => "Anthropic",
    }
}

/// Map a rig error from a call to `model` to a job outcome. `secrets` are the key values to
/// scrub.
pub fn classify_call(p: Provider, model: &str, e: &ProviderError, secrets: &[&str]) -> CallError {
    let name = provider_name(p);
    let mk = |kind, status, retry_after, detail: String| CallError {
        kind,
        status,
        retry_after,
        detail: scrub(&detail, secrets),
    };
    if let Some(code) = e.provider_response_status().map(|s| s.as_u16()) {
        let body = e.provider_response_body().unwrap_or_default();
        let retry_after = e
            .provider_response_headers()
            .and_then(|h| h.get("retry-after"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let (kind, detail) = classify_status(name, model, code, body);
        return mk(kind, Some(code), retry_after, detail);
    }
    match e {
        ProviderError::Http(inner) => {
            let (timeout, connect, chain) = transport_facts(inner);
            let (s, d) = classify_transport(timeout, connect, &chain);
            let kind = match s {
                ConnStatus::TimedOut => CallFail::TimedOut,
                _ => CallFail::NoConnection,
            };
            mk(kind, None, None, d)
        }
        ProviderError::Truncated => mk(CallFail::NoConnection, None, None, "No connection".into()),
        other => mk(
            CallFail::BadResponse,
            None,
            None,
            format!("Unexpected reply from {name}: {}", what(other)),
        ),
    }
}

/// A non-2xx reply's kind and user-facing detail. `model` is the model the call asked for.
/// A 403 or 404 about the model (no access, not found) is the model's problem, not the key's
/// (D1): only a 401, or a 403 that names the key or authentication, rejects the key.
fn classify_status(name: &str, model: &str, code: u16, body: &str) -> (CallFail, String) {
    let lower = body.to_ascii_lowercase();
    let too_large = || (CallFail::TooLarge, format!("Segment too large for {name}"));
    let names_model = ["model", "json_schema", "response_format", "output_config"]
        .iter()
        .any(|w| lower.contains(w))
        || lower.contains("responsejsonschema");
    let model_access = lower.contains("model_not_found")
        || lower.contains("does not have access")
        || (lower.contains("model") && (lower.contains("not found") || lower.contains("access")))
        || (!model.is_empty() && lower.contains(&model.to_ascii_lowercase()));
    let names_key = ["key", "auth", "credential"]
        .iter()
        .any(|w| lower.contains(w));
    let key_rejected = || {
        (
            CallFail::KeyRejected,
            format!("{name} rejected the key ({code})"),
        )
    };
    match code {
        401 => key_rejected(),
        403 | 404 if model_access => {
            let m = if model.is_empty() {
                "this model"
            } else {
                model
            };
            (
                CallFail::ModelRejected,
                format!("Your {name} account can't use {m}"),
            )
        }
        403 if names_key => key_rejected(),
        400 if body.contains("API_KEY_INVALID") => key_rejected(),
        413 => too_large(),
        400 if lower.contains("request payload size") || lower.contains("too large") => too_large(),
        400 | 404 if names_model => (CallFail::ModelRejected, classify_http(code, body).1),
        408 | 425 | 429 | 500..=599 => (CallFail::Retryable, classify_http(code, body).1),
        _ => (CallFail::Http, classify_http(code, body).1),
    }
}

/// A short, URL-free description of a failure that is not HTTP (JSON, decoding).
fn what(e: &ProviderError) -> String {
    match e {
        ProviderError::Json(j) => format!("JSON: {j}"),
        ProviderError::Response(m) | ProviderError::Provider(m) => m.clone(),
        other => format!("{:?}", other.kind()),
    }
}

/// `(is_timeout, is_connect, source chain)` of a transport error. The chain starts below
/// reqwest's own error, whose Display carries the URL.
fn transport_facts(e: &http_client::Error) -> (bool, bool, String) {
    let reqwest_err = match e {
        http_client::Error::Instance(b) => b.downcast_ref::<reqwest::Error>(),
        _ => None,
    };
    let Some(r) = reqwest_err else {
        return (false, false, "transport".into());
    };
    let mut parts = Vec::new();
    let mut src = std::error::Error::source(r);
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    (r.is_timeout(), r.is_connect(), parts.join(": "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_job_outcomes() {
        let k = |code, body| classify_status("Gemini", "m", code, body).0;
        assert_eq!(k(401, ""), CallFail::KeyRejected);
        assert_eq!(k(403, ""), CallFail::Http);
        assert_eq!(k(403, "API key not valid"), CallFail::KeyRejected);
        assert_eq!(k(400, "API_KEY_INVALID"), CallFail::KeyRejected);
        assert_eq!(k(413, ""), CallFail::TooLarge);
        assert_eq!(
            k(400, "Request payload size exceeds the limit"),
            CallFail::TooLarge
        );
        assert_eq!(k(400, "model foo not found"), CallFail::ModelRejected);
        assert_eq!(
            k(404, "Invalid responseJsonSchema"),
            CallFail::ModelRejected
        );
        assert_eq!(k(400, "bad"), CallFail::Http);
        for code in [408, 425, 429, 500, 503, 599] {
            assert_eq!(k(code, ""), CallFail::Retryable, "{code}");
        }
        assert_eq!(k(302, ""), CallFail::Http);
        assert_eq!(
            classify_status("OpenAI", "m", 401, "").1,
            "OpenAI rejected the key (401)"
        );
        assert_eq!(
            classify_status("Gemini", "m", 413, "").1,
            "Segment too large for Gemini"
        );
    }

    /// D1: a 403 for a model the account can't use is not a key problem.
    #[test]
    fn model_access_is_not_a_key_rejection() {
        let access = r#"{"error":{"message":"Project `proj_abc123` does not have access to model `gpt-transcribe`","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
        let (kind, detail) = classify_status("OpenAI", "gpt-transcribe", 403, access);
        assert_eq!(kind, CallFail::ModelRejected);
        assert_eq!(detail, "Your OpenAI account can't use gpt-transcribe");
        let gone = r#"{"error":{"message":"The model `gpt-x` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
        assert_eq!(
            classify_status("OpenAI", "gpt-x", 404, gone),
            (
                CallFail::ModelRejected,
                "Your OpenAI account can't use gpt-x".into()
            )
        );
        let gemini = r#"{"error":{"code":404,"message":"models/gemini-x is not found for API version v1beta","status":"NOT_FOUND"}}"#;
        assert_eq!(
            classify_status("Gemini", "gemini-x", 404, gemini).1,
            "Your Gemini account can't use gemini-x"
        );
        assert_eq!(
            classify_status("OpenAI", "", 403, access).1,
            "Your OpenAI account can't use this model"
        );
        let bad_key = r#"{"error":{"message":"Incorrect API key provided: sk-proj-****abcd. You can find your API key at https://platform.openai.com/account/api-keys.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;
        assert_eq!(
            classify_status("OpenAI", "gpt-transcribe", 401, bad_key),
            (
                CallFail::KeyRejected,
                "OpenAI rejected the key (401)".into()
            )
        );
        let denied = r#"{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API.","status":"PERMISSION_DENIED"}}"#;
        assert_eq!(
            classify_status("Gemini", "gemini-3.8-flash", 403, denied).0,
            CallFail::KeyRejected
        );
        let region = r#"{"error":{"code":"unsupported_country_region_territory","message":"Country, region, or territory not supported"}}"#;
        assert_eq!(
            classify_status("OpenAI", "gpt-4o-transcribe", 403, region).0,
            CallFail::Http
        );
    }

    #[test]
    fn errors_without_a_reply() {
        let e = ProviderError::Truncated;
        let c = classify_call(Provider::OpenAi, "m", &e, &[]);
        assert_eq!(
            (c.kind, c.detail.as_str()),
            (CallFail::NoConnection, "No connection")
        );
        let e = ProviderError::Response("no candidates".into());
        let c = classify_call(Provider::Gemini, "m", &e, &[]);
        assert_eq!(c.kind, CallFail::BadResponse);
        assert_eq!(c.detail, "Unexpected reply from Gemini: no candidates");
        let j = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let c = classify_call(
            Provider::Anthropic,
            "m",
            &ProviderError::Json(j.into()),
            &[],
        );
        assert!(
            c.detail
                .starts_with("Unexpected reply from Anthropic: JSON:"),
            "{c:?}"
        );
        let c = classify_call(
            Provider::Anthropic,
            "m",
            &ProviderError::Url(reqwest::Url::parse("").unwrap_err()),
            &[],
        );
        assert_eq!(c.detail, "Unexpected reply from Anthropic: Url");
        let other = http_client::Error::NoHeaders;
        let c = classify_call(
            Provider::OpenAi,
            "m",
            &ProviderError::Http(other.into()),
            &[],
        );
        assert_eq!(c.detail, "No connection: transport");
    }

    #[test]
    fn provider_names() {
        let names = Provider::ALL.map(provider_name);
        assert_eq!(names, ["OpenAI", "Gemini", "Anthropic"]);
    }
}
