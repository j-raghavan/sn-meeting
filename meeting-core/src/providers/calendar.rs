//! F18-FR3: fetching one calendar link (here because only the providers module names reqwest,
//! R8; `calendar::fetch` re-exports it). HTTPS GET only, no credentials and no cookies; the
//! client is built by `providers::http_client` (bundled roots, HTTPS only, no redirect
//! following) and this module follows at most 3 HTTPS-only redirects itself, so the provider
//! client still follows none (F1-FR8). Every message leaving here is URL-free: reqwest errors
//! lose their URL (`without_url`) and the caller scrubs the link's secrets as well.
use super::Http;
use crate::providers;
use rig_reqwest::reqwest::{self, StatusCode, Url, header};
use std::io::Read;
use std::time::Duration;

/// F18-FR3's limits.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 3;

/// How a fetch is bounded. `Default` is the device's; tests shorten the timeouts and allow the
/// mock server's `http:`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchCfg {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_bytes: usize,
    /// Tests only: the mock server speaks `http:`. On the device a `http:` hop is refused.
    pub allow_http: bool,
}

impl Default for FetchCfg {
    fn default() -> Self {
        FetchCfg {
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            max_bytes: MAX_BYTES,
            allow_http: false,
        }
    }
}

/// The cache's validators, sent back as `If-None-Match` / `If-Modified-Since`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// What a fetch came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// 2xx: the feed's text and its new validators.
    Body {
        text: String,
        validators: Validators,
    },
    /// 304: the cache is current.
    NotModified,
    /// 401, 403 or 404: "This calendar link no longer works" (EC23).
    Gone(u16),
    /// Anything else, as a short URL-free reason.
    Failed(String),
}

/// The message EC23 shows for a revoked or reset link.
pub const GONE_TEXT: &str = "This calendar link no longer works";

/// F18-FR3's redirect rule: at most [`MAX_REDIRECTS`] hops, HTTPS only (an `http:` Location is
/// refused), cross-host allowed (Google's `www.google.com` form 302s to `calendar.google.com`).
pub fn next_hop(
    current: &Url,
    location: &str,
    followed: usize,
    allow_http: bool,
) -> Result<Url, &'static str> {
    if followed >= MAX_REDIRECTS {
        return Err("Too many redirects");
    }
    let next = current.join(location.trim()).map_err(|_| "Bad redirect")?;
    match next.scheme() {
        "https" => Ok(next),
        "http" if allow_http => Ok(next),
        "http" => Err("Redirect to http:// refused"),
        _ => Err("Bad redirect"),
    }
}

/// Fetch `url` within `cfg.request_timeout` overall.
pub async fn fetch(url: &str, v: &Validators, cfg: &FetchCfg) -> Fetched {
    let http = match providers::http_client(cfg.connect_timeout, !cfg.allow_http) {
        Ok(h) => h,
        Err(_) => return Fetched::Failed("Couldn't start the connection".into()),
    };
    let Ok(start) = Url::parse(url) else {
        return Fetched::Failed("Not a link".into());
    };
    match tokio::time::timeout(cfg.request_timeout, follow(&http, start, v, cfg)).await {
        Ok(f) => f,
        Err(_) => Fetched::Failed("Timed out".into()),
    }
}

async fn follow(http: &Http, mut url: Url, v: &Validators, cfg: &FetchCfg) -> Fetched {
    if url.scheme() != "https" && !cfg.allow_http {
        return Fetched::Failed("Only https:// links are read".into());
    }
    let mut followed = 0;
    loop {
        let resp = match request(http, url.clone(), v).send().await {
            Ok(r) => r,
            Err(e) => return Fetched::Failed(transport(e)),
        };
        let status = resp.status();
        if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
            let loc = resp
                .headers()
                .get(header::LOCATION)
                .and_then(|l| l.to_str().ok())
                .map(str::to_owned);
            let Some(loc) = loc else {
                return Fetched::Failed(format!("HTTP {}", status.as_u16()));
            };
            match next_hop(&url, &loc, followed, cfg.allow_http) {
                Ok(n) => {
                    url = n;
                    followed += 1;
                    continue;
                }
                Err(e) => return Fetched::Failed(e.into()),
            }
        }
        return match status.as_u16() {
            304 => Fetched::NotModified,
            401 | 403 | 404 => Fetched::Gone(status.as_u16()),
            200..=299 => body(resp, cfg.max_bytes).await,
            c => Fetched::Failed(format!("HTTP {c}")),
        };
    }
}

/// A GET with no credentials (none exist; reqwest keeps no cookies) and the cache's validators.
fn request(http: &Http, url: Url, v: &Validators) -> reqwest::RequestBuilder {
    let mut r = http
        .get(url)
        .header(header::ACCEPT, "text/calendar, */*;q=0.5")
        .header(header::ACCEPT_ENCODING, "gzip")
        .header(
            header::USER_AGENT,
            concat!("Meetings/", env!("CARGO_PKG_VERSION")),
        );
    if let Some(e) = &v.etag {
        r = r.header(header::IF_NONE_MATCH, e.as_str());
    }
    if let Some(m) = &v.last_modified {
        r = r.header(header::IF_MODIFIED_SINCE, m.as_str());
    }
    r
}

/// The body, at most `cap` bytes on the wire and after gzip (EC25), as text.
async fn body(mut resp: reqwest::Response, cap: usize) -> Fetched {
    let header_text = |name| {
        resp.headers()
            .get(name)
            .and_then(|h: &header::HeaderValue| h.to_str().ok())
            .map(str::to_owned)
    };
    let validators = Validators {
        etag: header_text(header::ETAG).filter(|e| e.len() <= 256),
        last_modified: header_text(header::LAST_MODIFIED).filter(|e| e.len() <= 64),
    };
    let encoding = header_text(header::CONTENT_ENCODING)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Fetched::Failed(too_large());
    }
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                if buf.len() + c.len() > cap {
                    return Fetched::Failed(too_large());
                }
                buf.extend_from_slice(&c);
            }
            Ok(None) => break,
            Err(e) => return Fetched::Failed(transport(e)),
        }
    }
    let bytes = match encoding.trim() {
        "" | "identity" => buf,
        "gzip" | "x-gzip" => match gunzip(&buf, cap) {
            Ok(b) => b,
            Err(e) => return Fetched::Failed(e),
        },
        _ => return Fetched::Failed("Unsupported encoding".into()),
    };
    Fetched::Body {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        validators,
    }
}

fn too_large() -> String {
    format!("Calendar larger than {} MB", MAX_BYTES / (1024 * 1024))
}

/// Inflate at most `cap` bytes; more is a bomb, not a calendar.
pub fn gunzip(bytes: &[u8], cap: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut d = flate2::read::GzDecoder::new(bytes).take(cap as u64 + 1);
    d.read_to_end(&mut out)
        .map_err(|_| "Unreadable compressed reply".to_owned())?;
    if out.len() > cap {
        return Err(too_large());
    }
    Ok(out)
}

/// A transport failure in the provider client's words, with the URL removed first.
fn transport(e: reqwest::Error) -> String {
    let (timeout, connect) = (e.is_timeout(), e.is_connect());
    let e = e.without_url();
    let mut chain = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        chain.push_str(": ");
        chain.push_str(&s.to_string());
        src = s.source();
    }
    providers::classify_transport(timeout, connect, &chain).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn redirects_stay_on_https_and_stop_at_three() {
        let g = u("https://www.google.com/calendar/ical/x/basic.ics");
        let n = next_hop(
            &g,
            "https://calendar.google.com/calendar/ical/x/basic.ics",
            0,
            false,
        );
        assert_eq!(n.unwrap().host_str(), Some("calendar.google.com"));
        assert_eq!(
            next_hop(&g, "/other.ics", 2, false).unwrap().path(),
            "/other.ics"
        );
        assert_eq!(next_hop(&g, "/x", 3, false), Err("Too many redirects"));
        assert_eq!(
            next_hop(&g, "http://calendar.google.com/x", 0, false),
            Err("Redirect to http:// refused")
        );
        assert!(next_hop(&u("http://127.0.0.1:1/a"), "/b", 0, true).is_ok());
        assert_eq!(next_hop(&g, "ftp://x/y", 0, true), Err("Bad redirect"));
        assert_eq!(next_hop(&g, "https://[bad", 0, false), Err("Bad redirect"));
    }

    #[test]
    fn gzip_is_capped() {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(&vec![b'A'; 10_000]).unwrap();
        let z = e.finish().unwrap();
        assert_eq!(gunzip(&z, 10_000).unwrap().len(), 10_000);
        assert!(
            gunzip(&z, 9_999)
                .unwrap_err()
                .starts_with("Calendar larger")
        );
        assert_eq!(
            gunzip(b"nope", 10).unwrap_err(),
            "Unreadable compressed reply"
        );
    }

    #[test]
    fn the_device_limits_are_the_spec() {
        let c = FetchCfg::default();
        assert_eq!(c.connect_timeout, Duration::from_secs(15));
        assert_eq!(c.request_timeout, Duration::from_secs(30));
        assert_eq!(c.max_bytes, 5 * 1024 * 1024);
        assert!(!c.allow_http);
    }

    #[test]
    fn a_device_fetch_refuses_http_and_junk() {
        let cfg = FetchCfg::default();
        let r = crate::runtime::block_on(fetch(
            "http://127.0.0.1:9/x.ics",
            &Validators::default(),
            &cfg,
        ));
        assert_eq!(
            r.unwrap(),
            Fetched::Failed("Only https:// links are read".into())
        );
        let r = crate::runtime::block_on(fetch("not a url", &Validators::default(), &cfg));
        assert_eq!(r.unwrap(), Fetched::Failed("Not a link".into()));
    }
}
