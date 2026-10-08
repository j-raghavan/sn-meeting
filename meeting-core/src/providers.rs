//! Provider HTTP. The only module that names rig or reqwest (R8).
use rig_reqwest::reqwest;
use std::time::Duration;

/// Test connection's whole-call budget (F10-FR5).
pub const TEST_TIMEOUT: Duration = Duration::from_secs(15);

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
        .connect_timeout(connect_timeout)
        .build()
        .map_err(|e| e.without_url().to_string())
}

/// The rig transport over our client (M4: `OpenAIConfig::new(k).connect(rig_http(c))`).
pub fn rig_http(client: reqwest::Client) -> rig_reqwest::ReqwestClient {
    rig_reqwest::ReqwestClient::from(client)
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
}
