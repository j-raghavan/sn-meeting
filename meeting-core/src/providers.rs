//! Provider HTTP. The only module that names rig or reqwest (R8).
use crate::keyfile::{Provider, ProviderKey};
use crate::scrub::scrub;
use rig_reqwest::reqwest;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};

mod calls;
pub use calls::{CallError, CallFail, Http, classify_call, provider_name};

/// Test connection's whole-call budget (F10-FR5).
pub const TEST_TIMEOUT: Duration = Duration::from_secs(15);
/// A model listing never needs more pages than this; stop rather than loop on a bad cursor.
const MAX_PAGES: usize = 10;
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// An error body is only mined for one message line; a model page is a few hundred KB at most.
const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;

/// API roots. `Default` is the real hosts; tests point every field at a wiremock server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    /// Includes the `/v1` segment, as rig's `OpenAIConfig::with_base_url` expects.
    pub openai: String,
    pub gemini: String,
    pub anthropic: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            openai: "https://api.openai.com/v1".into(),
            gemini: "https://generativelanguage.googleapis.com".into(),
            anthropic: "https://api.anthropic.com".into(),
        }
    }
}

/// The one reqwest client: Mozilla roots via `tls_certs_only`, so reqwest builds a plain rustls
/// `WebPkiServerVerifier` and never reaches rustls-platform-verifier (reqwest async_impl/client.rs
/// `tls_certs_only`). Only the connect phase has a reqwest timeout; callers bound the whole call
/// with one `tokio::time::timeout`.
pub fn http_client(connect_timeout: Duration, https_only: bool) -> Result<reqwest::Client, String> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|der| reqwest::Certificate::from_der(der.as_ref()).ok());
    reqwest::Client::builder()
        .tls_certs_only(roots)
        .https_only(https_only)
        // Never follow a redirect: reqwest strips Authorization across origins, but not our
        // x-api-key / x-goog-api-key headers. A 3xx is reported as an HTTP error instead.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect_timeout)
        .build()
        .map_err(|e| e.without_url().to_string())
}

/// The rig transport over our client (M4: `OpenAIConfig::new(k).connect(rig_http(c))`).
pub fn rig_http(client: reqwest::Client) -> rig_reqwest::ReqwestClient {
    rig_reqwest::ReqwestClient::from(client)
}

/// The outcome of Test connection, serialised for the shell as snake_case JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnStatus {
    Connected,
    KeyRejected,
    NoConnection,
    TlsError,
    TimedOut,
    HttpError,
}

/// `nativeTestConnection`'s reply. Every string in it has been scrubbed of the key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConnectionResult {
    pub ok: bool,
    pub status: ConnStatus,
    pub ms: u64,
    pub provider: &'static str,
    pub model: String,
    /// Whether `model` is in the listing; `None` when nothing was listed.
    pub model_found: Option<bool>,
    pub detail: String,
}

/// F10-FR5 / F1-FR9: list models with `key` (no audio, no cost), within `timeout` overall.
pub async fn test_connection(
    http: &reqwest::Client,
    ep: &Endpoints,
    key: &ProviderKey,
    model: &str,
    timeout: Duration,
) -> ConnectionResult {
    let started = Instant::now();
    let outcome = tokio::time::timeout(timeout, list_ids(http, ep, key))
        .await
        .unwrap_or_else(|_| Err(Fail::Transport(ConnStatus::TimedOut, "Timed out".into())));
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (status, model_found, detail) = match outcome {
        Ok(ids) => {
            let found = ids.iter().any(|id| id == model);
            let mut d = format!("Connected · {ms} ms");
            if !found {
                d.push_str(&format!(" · model {model} not found"));
            }
            (ConnStatus::Connected, Some(found), d)
        }
        Err(Fail::Transport(s, d)) => (s, None, d),
        Err(Fail::Http(code, body)) => {
            let (s, d) = classify_http(code, &body);
            (s, None, d)
        }
    };
    let secret = [key.expose()];
    ConnectionResult {
        ok: status == ConnStatus::Connected,
        status,
        ms,
        provider: key.provider.id(),
        model: scrub(model, &secret),
        model_found,
        detail: scrub(&detail, &secret),
    }
}

enum Fail {
    Transport(ConnStatus, String),
    Http(u16, String),
}

/// Every model id the key can see, following at most `MAX_PAGES` pages.
async fn list_ids(
    http: &reqwest::Client,
    ep: &Endpoints,
    key: &ProviderKey,
) -> Result<Vec<String>, Fail> {
    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let resp = request(http, ep, key, cursor.as_deref())?
            .send()
            .await
            .map_err(transport)?;
        let code = resp.status().as_u16();
        let ok = (200..300).contains(&code);
        let (body, cut) =
            read_body(resp, if ok { MAX_LIST_BYTES } else { MAX_ERROR_BYTES }).await?;
        if !ok {
            return Err(Fail::Http(code, body)); // only its first line or JSON message is shown
        }
        let unexpected = || Fail::Http(code, "unexpected response".into());
        if cut {
            return Err(unexpected());
        }
        let (page, next) = parse_page(key.provider, &body).ok_or_else(unexpected)?;
        ids.extend(page);
        match next {
            Some(n) => cursor = Some(n),
            None => break,
        }
    }
    Ok(ids)
}

/// Read at most `cap` bytes of the body, decoding lossily; `true` when the body was longer.
async fn read_body(mut resp: reqwest::Response, cap: usize) -> Result<(String, bool), Fail> {
    let mut buf: Vec<u8> = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await.map_err(transport)? {
        let room = cap - buf.len();
        if chunk.len() > room {
            buf.extend_from_slice(&chunk[..room]);
            cut = true;
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), cut))
}

/// One listing request. Keys go in headers only: never in the URL (rig's Gemini wire puts
/// `key=` in the query, which is why Test connection does not use rig here).
fn request(
    http: &reqwest::Client,
    ep: &Endpoints,
    key: &ProviderKey,
    cursor: Option<&str>,
) -> Result<reqwest::RequestBuilder, Fail> {
    Ok(match key.provider {
        Provider::OpenAi => http
            .get(url(&format!("{}/models", ep.openai), &[], None)?)
            .bearer_auth(key.expose()),
        // UNVERIFIED against the live API: limit=1000 (Anthropic documents a 1000 maximum).
        Provider::Anthropic => http
            .get(url(
                &format!("{}/v1/models", ep.anthropic),
                &[("limit", "1000")],
                cursor.map(|c| ("after_id", c)),
            )?)
            .header("x-api-key", key.expose())
            .header("anthropic-version", ANTHROPIC_VERSION),
        Provider::Gemini => http
            .get(url(
                &format!("{}/v1beta/models", ep.gemini),
                &[("pageSize", "1000")],
                cursor.map(|c| ("pageToken", c)),
            )?)
            .header("x-goog-api-key", key.expose()),
    })
}

/// `base?q&cursor`, query-encoded (reqwest's `query` feature is off).
fn url(base: &str, q: &[(&str, &str)], cursor: Option<(&str, &str)>) -> Result<reqwest::Url, Fail> {
    let pairs = q.iter().copied().chain(cursor);
    reqwest::Url::parse_with_params(base, pairs).map_err(|_| {
        Fail::Transport(
            ConnStatus::NoConnection,
            "No connection: bad endpoint".into(),
        )
    })
}

/// A page's model ids (Gemini's `models/` prefix dropped) and the next cursor, if any.
fn parse_page(provider: Provider, body: &str) -> Option<(Vec<String>, Option<String>)> {
    let page: Value = serde_json::from_str(body).ok()?;
    let (items, field, next) = match provider {
        Provider::OpenAi => ("data", "id", None),
        Provider::Anthropic => (
            "data",
            "id",
            page["last_id"]
                .as_str()
                .filter(|_| page["has_more"] == true),
        ),
        Provider::Gemini => ("models", "name", page["nextPageToken"].as_str()),
    };
    let ids = page[items]
        .as_array()?
        .iter()
        .filter_map(|m| m[field].as_str())
        .map(|id| id.strip_prefix("models/").unwrap_or(id).to_owned())
        .collect();
    Some((ids, next.filter(|n| !n.is_empty()).map(str::to_owned)))
}

fn transport(e: reqwest::Error) -> Fail {
    let (timeout, connect) = (e.is_timeout(), e.is_connect());
    let e = e.without_url();
    let mut chain = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        chain.push_str(": ");
        chain.push_str(&s.to_string());
        src = s.source();
    }
    let (status, detail) = classify_transport(timeout, connect, &chain);
    Fail::Transport(status, detail)
}

/// Map a transport failure. `chain` is the error and its sources, URL already removed.
pub fn classify_transport(timeout: bool, connect: bool, chain: &str) -> (ConnStatus, String) {
    if timeout {
        (ConnStatus::TimedOut, "Timed out".into())
    } else if chain.contains("certificate") {
        let d = format!("Certificate check failed: {chain}");
        (ConnStatus::TlsError, d)
    } else if connect {
        (ConnStatus::NoConnection, "No connection".into())
    } else {
        (ConnStatus::NoConnection, format!("No connection: {chain}"))
    }
}

/// Map a non-2xx reply (or an unreadable 2xx) to a status and a short detail.
pub fn classify_http(code: u16, body: &str) -> (ConnStatus, String) {
    // UNVERIFIED against the live API: Gemini answers a bad key with 400 + API_KEY_INVALID.
    if code == 401 || code == 403 || (code == 400 && body.contains("API_KEY_INVALID")) {
        return (ConnStatus::KeyRejected, format!("Key rejected ({code})"));
    }
    let json_message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned));
    let text_line = || {
        let line = body.trim().lines().next()?.trim();
        (!line.starts_with('<')).then(|| line.to_owned())
    };
    let detail = match json_message.or_else(text_line) {
        Some(m) => format!("HTTP {code}: {m}"),
        None => format!("HTTP {code}"),
    };
    (ConnStatus::HttpError, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_endpoints_are_the_real_hosts() {
        let ep = Endpoints::default();
        assert_eq!(ep.openai, "https://api.openai.com/v1");
        assert_eq!(ep.gemini, "https://generativelanguage.googleapis.com");
        assert_eq!(ep.anthropic, "https://api.anthropic.com");
    }

    #[test]
    fn the_mozilla_roots_all_parse() {
        let n = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .filter(|der| reqwest::Certificate::from_der(der.as_ref()).is_ok())
            .count();
        assert!(n > 100, "only {n} roots parsed");
        assert!(http_client(TEST_TIMEOUT, true).is_ok());
    }

    #[test]
    fn transport_failures_map_to_statuses() {
        let c = |t, k, chain| classify_transport(t, k, chain);
        assert_eq!(
            c(true, true, "x"),
            (ConnStatus::TimedOut, "Timed out".into())
        );
        // Only reachable live (a bad or missing root); rustls names the certificate.
        let tls = "error sending request: client error (Connect): invalid peer certificate: UnknownIssuer";
        assert_eq!(
            c(false, true, tls),
            (
                ConnStatus::TlsError,
                format!("Certificate check failed: {tls}")
            )
        );
        assert_eq!(
            c(false, true, "dns error"),
            (ConnStatus::NoConnection, "No connection".into())
        );
        assert_eq!(
            c(false, false, "builder"),
            (ConnStatus::NoConnection, "No connection: builder".into())
        );
    }

    #[test]
    fn http_failures_map_to_statuses() {
        assert_eq!(
            classify_http(401, ""),
            (ConnStatus::KeyRejected, "Key rejected (401)".into())
        );
        assert_eq!(
            classify_http(400, "API_KEY_INVALID").0,
            ConnStatus::KeyRejected
        );
        assert_eq!(
            classify_http(400, r#"{"error":{"message":"bad model"}}"#).1,
            "HTTP 400: bad model"
        );
        assert_eq!(
            classify_http(500, "  \n"),
            (ConnStatus::HttpError, "HTTP 500".into())
        );
    }

    #[test]
    fn statuses_serialise_as_snake_case() {
        let s = serde_json::to_string(&[ConnStatus::KeyRejected, ConnStatus::TlsError]).unwrap();
        assert_eq!(s, r#"["key_rejected","tls_error"]"#);
    }
}
