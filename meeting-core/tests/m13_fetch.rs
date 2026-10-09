//! F18-FR3: the calendar fetch against a local mock server: validators, 304, redirects (same
//! and cross host, at most 3), 401/403/404, oversize, gzip, timeouts. The device's HTTPS-only
//! rule for redirects is unit-tested in `providers::calendar` (the mock speaks http).
use meeting::calendar::fetch::{self, FetchCfg, Fetched, Validators};
use std::io::Write;
use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cfg() -> FetchCfg {
    FetchCfg {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(2),
        max_bytes: 64 * 1024,
        allow_http: true,
    }
}

const ICS: &str = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";

async fn get(url: &str, v: &Validators, c: &FetchCfg) -> Fetched {
    let (url, v, c) = (url.to_owned(), v.clone(), *c);
    tokio::task::spawn_blocking(move || {
        meeting::runtime::block_on(fetch::fetch(&url, &v, &c)).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn etag_then_304_sends_the_validators() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cal/basic.ics"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/cal/basic.ics"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .insert_header("last-modified", "Thu, 08 Oct 2026 10:00:00 GMT")
                .set_body_string(ICS),
        )
        .mount(&s)
        .await;
    let url = format!("{}/cal/basic.ics", s.uri());
    let first = get(&url, &Validators::default(), &cfg()).await;
    let Fetched::Body { text, validators } = first else {
        panic!("{first:?}");
    };
    assert_eq!(text, ICS);
    assert_eq!(validators.etag.as_deref(), Some("\"v1\""));
    assert_eq!(
        validators.last_modified.as_deref(),
        Some("Thu, 08 Oct 2026 10:00:00 GMT")
    );
    assert_eq!(get(&url, &validators, &cfg()).await, Fetched::NotModified);
    let reqs = s.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    let h = &reqs[1].headers;
    assert_eq!(
        h.get("if-modified-since").unwrap(),
        "Thu, 08 Oct 2026 10:00:00 GMT"
    );
    assert!(
        h.get("authorization").is_none() && h.get("cookie").is_none(),
        "no credentials"
    );
    assert_eq!(h.get("accept-encoding").unwrap(), "gzip");
}

#[tokio::test]
async fn redirects_same_and_cross_host_up_to_three() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    // a: /1 → /2 (same host) → b (cross host, as www.google.com → calendar.google.com).
    let b_port = b.address().port();
    Mock::given(path("/1"))
        .respond_with(ResponseTemplate::new(301).insert_header("location", "/2"))
        .mount(&a)
        .await;
    Mock::given(path("/2"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("http://localhost:{b_port}/3").as_str()),
        )
        .mount(&a)
        .await;
    Mock::given(path("/3"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ICS))
        .mount(&b)
        .await;
    let r = get(&format!("{}/1", a.uri()), &Validators::default(), &cfg()).await;
    assert!(matches!(r, Fetched::Body { .. }), "{r:?}");
    // A loop: more than three hops is refused.
    Mock::given(path("/loop"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/loop"))
        .mount(&a)
        .await;
    let r = get(&format!("{}/loop", a.uri()), &Validators::default(), &cfg()).await;
    assert_eq!(r, Fetched::Failed("Too many redirects".into()));
    let hops = a
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/loop")
        .count();
    assert_eq!(hops, 4, "the first request and three redirects");
    // A redirect with no Location, and one to a scheme that isn't http(s).
    Mock::given(path("/noloc"))
        .respond_with(ResponseTemplate::new(302))
        .mount(&a)
        .await;
    let r = get(
        &format!("{}/noloc", a.uri()),
        &Validators::default(),
        &cfg(),
    )
    .await;
    assert_eq!(r, Fetched::Failed("HTTP 302".into()));
    Mock::given(path("/ftp"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "ftp://x/y"))
        .mount(&a)
        .await;
    let r = get(&format!("{}/ftp", a.uri()), &Validators::default(), &cfg()).await;
    assert_eq!(r, Fetched::Failed("Bad redirect".into()));
}

#[tokio::test]
async fn gone_failed_oversize_and_gzip() {
    let s = MockServer::start().await;
    for (p, code) in [("/401", 401), ("/403", 403), ("/404", 404), ("/500", 500)] {
        Mock::given(path(p))
            .respond_with(ResponseTemplate::new(code))
            .mount(&s)
            .await;
    }
    let v = Validators::default();
    assert_eq!(
        get(&format!("{}/401", s.uri()), &v, &cfg()).await,
        Fetched::Gone(401)
    );
    assert_eq!(
        get(&format!("{}/403", s.uri()), &v, &cfg()).await,
        Fetched::Gone(403)
    );
    assert_eq!(
        get(&format!("{}/404", s.uri()), &v, &cfg()).await,
        Fetched::Gone(404)
    );
    assert_eq!(
        get(&format!("{}/500", s.uri()), &v, &cfg()).await,
        Fetched::Failed("HTTP 500".into())
    );
    Mock::given(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(70 * 1024)))
        .mount(&s)
        .await;
    let r = get(&format!("{}/big", s.uri()), &v, &cfg()).await;
    assert_eq!(r, Fetched::Failed("Calendar larger than 5 MB".into()));
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(ICS.as_bytes()).unwrap();
    Mock::given(path("/gz"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-encoding", "gzip")
                .set_body_bytes(z.finish().unwrap()),
        )
        .mount(&s)
        .await;
    let r = get(&format!("{}/gz", s.uri()), &v, &cfg()).await;
    assert!(
        matches!(&r, Fetched::Body { text, .. } if text == ICS),
        "{r:?}"
    );
    Mock::given(path("/br"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-encoding", "br")
                .set_body_string("x"),
        )
        .mount(&s)
        .await;
    let r = get(&format!("{}/br", s.uri()), &v, &cfg()).await;
    assert_eq!(r, Fetched::Failed("Unsupported encoding".into()));
}

#[tokio::test]
async fn timeouts_and_no_connection() {
    let s = MockServer::start().await;
    Mock::given(path("/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&s)
        .await;
    let short = FetchCfg {
        request_timeout: Duration::from_millis(300),
        ..cfg()
    };
    let r = get(&format!("{}/slow", s.uri()), &Validators::default(), &short).await;
    assert_eq!(r, Fetched::Failed("Timed out".into()));
    // Nothing listens on port 9 here.
    let r = get("http://127.0.0.1:9/x.ics", &Validators::default(), &cfg()).await;
    assert!(
        matches!(&r, Fetched::Failed(m) if m.starts_with("No connection")),
        "{r:?}"
    );
}
