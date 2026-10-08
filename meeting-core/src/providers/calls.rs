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
    /// 401/403, or Gemini's 400 `API_KEY_INVALID`.
    KeyRejected,
    /// 413, or Gemini's request-size 400: never retried.
    TooLarge,
    /// 408/425/429/5xx: one automatic retry.
    Retryable,
    /// A 400/404 naming the model or the output format: summarise retries once with the
    /// provider's default model (F4-FR3).
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
    let text = reply.map_err(|e| classify_call(key.provider, &e, &[key.expose()]))?;
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
    let r = reply.map_err(|e| classify_call(key.provider, &e, &secret))?;
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

/// Map a rig error to a job outcome. `secrets` are the key values to scrub.
pub fn classify_call(p: Provider, e: &ProviderError, secrets: &[&str]) -> CallError {
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
        let (kind, detail) = classify_status(name, code, body);
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

/// A non-2xx reply's kind and user-facing detail.
fn classify_status(name: &str, code: u16, body: &str) -> (CallFail, String) {
    let lower = body.to_ascii_lowercase();
    let too_large = || (CallFail::TooLarge, format!("Segment too large for {name}"));
    let names_model = ["model", "json_schema", "response_format", "output_config"]
        .iter()
        .any(|w| lower.contains(w))
        || lower.contains("responsejsonschema");
    match code {
        401 | 403 => (
            CallFail::KeyRejected,
            format!("{name} rejected the key ({code})"),
        ),
        400 if body.contains("API_KEY_INVALID") => (
            CallFail::KeyRejected,
            format!("{name} rejected the key ({code})"),
        ),
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
        let k = |code, body| classify_status("Gemini", code, body).0;
        assert_eq!(k(401, ""), CallFail::KeyRejected);
        assert_eq!(k(403, ""), CallFail::KeyRejected);
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
            classify_status("OpenAI", 401, "").1,
            "OpenAI rejected the key (401)"
        );
        assert_eq!(
            classify_status("Gemini", 413, "").1,
            "Segment too large for Gemini"
        );
    }

    #[test]
    fn errors_without_a_reply() {
        let e = ProviderError::Truncated;
        let c = classify_call(Provider::OpenAi, &e, &[]);
        assert_eq!(
            (c.kind, c.detail.as_str()),
            (CallFail::NoConnection, "No connection")
        );
        let e = ProviderError::Response("no candidates".into());
        let c = classify_call(Provider::Gemini, &e, &[]);
        assert_eq!(c.kind, CallFail::BadResponse);
        assert_eq!(c.detail, "Unexpected reply from Gemini: no candidates");
        let j = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let c = classify_call(Provider::Anthropic, &ProviderError::Json(j.into()), &[]);
        assert!(
            c.detail
                .starts_with("Unexpected reply from Anthropic: JSON:"),
            "{c:?}"
        );
        let c = classify_call(
            Provider::Anthropic,
            &ProviderError::Url(reqwest::Url::parse("").unwrap_err()),
            &[],
        );
        assert_eq!(c.detail, "Unexpected reply from Anthropic: Url");
        let other = http_client::Error::NoHeaders;
        let c = classify_call(Provider::OpenAi, &ProviderError::Http(other.into()), &[]);
        assert_eq!(c.detail, "No connection: transport");
    }

    #[test]
    fn provider_names() {
        let names = Provider::ALL.map(provider_name);
        assert_eq!(names, ["OpenAI", "Gemini", "Anthropic"]);
    }
}
