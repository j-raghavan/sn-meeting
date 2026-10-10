//! F18-FR3 / FR4 / FR6 / F18-AC2: refreshing links into the app-private cache against a mock
//! server: 200 with an ETag then 304 (the cache stays, `If-None-Match` is sent), a 404 ("no
//! longer works", the cached events remain), the refresh interval, manual-only, a recording's
//! first seconds, and links removed from the file.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::calendar::fetch::FetchCfg;
use serde_json::Value;
use std::time::Duration;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 2026-10-07 12:00 PDT: the fixtures' week.
const NOW: i64 = 1_791_399_600_000;
const MIN: i64 = 60_000;

fn ics() -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/calendar/google.ics",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

pub fn test_app(env: &Env) -> App {
    let mut app = env.app();
    app.calendar.cfg = FetchCfg {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(3),
        allow_http: true,
        ..FetchCfg::default()
    };
    app
}

fn write_links(env: &Env, text: &str) {
    let p = env.root.path().join("MyStyle/Meetings/calendar-links.txt");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn link_url(s: &MockServer, secret: &str) -> String {
    format!(
        "https://127.0.0.1:{}/calendar/ical/me%40example.com/{secret}/basic.ics",
        s.address().port()
    )
}

async fn refresh(app: &std::sync::Arc<App>, force: bool, now: i64) -> Value {
    let app = app.clone();
    tokio::task::spawn_blocking(move || app.calendar_refresh_at(force, now))
        .await
        .unwrap()
}

fn links(app: &App, now: i64) -> Vec<Value> {
    app.calendar_links_at(now)
}

#[tokio::test]
async fn etag_then_304_then_404_keeps_the_events() {
    let env = Env::new();
    let app = std::sync::Arc::new(test_app(&env));
    let s = MockServer::start().await;
    let url = link_url(&s, "private-aaaa1111bbbb2222");
    write_links(&env, &format!("Work = {url}\n"));
    Mock::given(path(
        "/calendar/ical/me%40example.com/private-aaaa1111bbbb2222/basic.ics",
    ))
    .and(header("if-none-match", "\"e1\""))
    .respond_with(ResponseTemplate::new(304))
    .up_to_n_times(1)
    .mount(&s)
    .await;
    Mock::given(path(
        "/calendar/ical/me%40example.com/private-aaaa1111bbbb2222/basic.ics",
    ))
    .respond_with(
        ResponseTemplate::new(200)
            .insert_header("etag", "\"e1\"")
            .set_body_string(ics()),
    )
    .up_to_n_times(1)
    .mount(&s)
    .await;
    let r = refresh(&app, false, NOW).await;
    assert_eq!(r["fetched"], 1, "{r}");
    assert_eq!(r["updatedAtMs"], NOW);
    assert_eq!(r["failed"], serde_json::json!([]));
    let l = &links(&app, NOW)[0];
    assert_eq!(l["name"], "Work", "X-WR-CALNAME");
    assert_eq!(l["status"], "ok");
    assert_eq!(l["skipped"], 2);
    let week = l["eventsThisWeek"].as_u64().unwrap();
    assert!(week >= 5, "{l}");
    let before = app.cached_events().len();
    assert!(before > 5);

    // The next refresh is not due yet; forced, it sends If-None-Match and keeps the cache.
    assert_eq!(refresh(&app, false, NOW + MIN).await["fetched"], 0);
    let r = refresh(&app, true, NOW + 2 * MIN).await;
    assert_eq!(r["fetched"], 1);
    assert_eq!(r["updatedAtMs"], NOW + 2 * MIN);
    assert_eq!(app.cached_events().len(), before);
    let reqs = s.received_requests().await.unwrap();
    assert_eq!(reqs[1].headers.get("if-none-match").unwrap(), "\"e1\"");

    // The link is reset in Google Calendar: 404. The events stay, with their age.
    Mock::given(path(
        "/calendar/ical/me%40example.com/private-aaaa1111bbbb2222/basic.ics",
    ))
    .respond_with(ResponseTemplate::new(404))
    .mount(&s)
    .await;
    let r = refresh(&app, true, NOW + 40 * MIN).await;
    assert_eq!(r["updatedAtMs"], NOW + 2 * MIN, "the last good refresh");
    let id = links(&app, NOW)[0]["linkId"].as_str().unwrap().to_owned();
    assert_eq!(r["failed"], serde_json::json!([id]));
    let l = &links(&app, NOW)[0];
    assert_eq!(l["status"], "gone");
    assert_eq!(l["reason"], "This calendar link no longer works (404)");
    assert_eq!(l["lastOk"], NOW + 2 * MIN);
    assert_eq!(l["lastAttempt"], NOW + 40 * MIN);
    assert_eq!(
        app.cached_events().len(),
        before,
        "F18-AC9: the cache still shows"
    );
}

#[tokio::test]
async fn the_interval_manual_only_and_a_recordings_first_seconds() {
    let env = Env::new();
    let app = std::sync::Arc::new(test_app(&env));
    let s = MockServer::start().await;
    Mock::given(path("/calendar/ical/me%40example.com/p1/basic.ics"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ics()))
        .mount(&s)
        .await;
    write_links(&env, &format!("{}\n", link_url(&s, "p1")));
    assert_eq!(refresh(&app, false, NOW).await["fetched"], 1);
    // Google sends no validators: due again after 30 minutes (the default).
    assert_eq!(refresh(&app, false, NOW + 29 * MIN).await["fetched"], 0);
    assert_eq!(refresh(&app, false, NOW + 30 * MIN).await["fetched"], 1);
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    // Manual only: never on its own, always on Refresh.
    let r = app.set_settings_json(r#"{"calendar":{"refreshMin":0}}"#);
    assert!(!r.contains("error"), "{r}");
    let r = refresh(&app, false, NOW + 300 * MIN).await;
    assert_eq!(
        (r["fetched"].as_u64(), r["deferred"].as_str()),
        (Some(0), Some("manual"))
    );
    assert_eq!(refresh(&app, true, NOW + 301 * MIN).await["fetched"], 1);
    // A bad interval is refused; 4 h is fine.
    let r: Value =
        serde_json::from_str(&app.set_settings_json(r#"{"calendar":{"refreshMin":7}}"#)).unwrap();
    assert_eq!(r["field"], "calendar.refreshMin");
    let r = app.set_settings_json(r#"{"calendar":{"refreshMin":240}}"#);
    assert!(!r.contains("error"), "{r}");
    // A recording that began 3 s ago is left alone.
    let started = NOW + 600 * MIN;
    let b = app.begin_json(started, &mic_json().to_string(), &audio_json().to_string());
    assert!(!b.contains("error"), "{b}");
    let r = refresh(&app, false, started + 3_000).await;
    assert_eq!(r["deferred"], "recording");
    let r = refresh(&app, false, started + 30_000).await;
    assert!(r.get("deferred").is_none(), "{r}");
    assert_eq!(r["fetched"], 1);
}

#[tokio::test]
async fn failures_keep_the_last_good_cache_and_removed_links_go() {
    let env = Env::new();
    let app = std::sync::Arc::new(test_app(&env));
    let s = MockServer::start().await;
    Mock::given(path("/calendar/ical/me%40example.com/a/basic.ics"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ics()))
        .up_to_n_times(1)
        .mount(&s)
        .await;
    Mock::given(path("/calendar/ical/me%40example.com/a/basic.ics"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>sign in</html>"))
        .up_to_n_times(1)
        .mount(&s)
        .await;
    Mock::given(path("/calendar/ical/me%40example.com/a/basic.ics"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&s)
        .await;
    let a = link_url(&s, "a");
    write_links(&env, &format!("{a}\n"));
    refresh(&app, true, NOW).await;
    let n = app.cached_events().len();
    let r = refresh(&app, true, NOW + MIN).await;
    assert_eq!(r["failed"].as_array().unwrap().len(), 1);
    assert_eq!(links(&app, NOW)[0]["reason"], "Not a calendar");
    refresh(&app, true, NOW + 2 * MIN).await;
    assert_eq!(links(&app, NOW)[0]["reason"], "HTTP 500");
    assert_eq!(app.cached_events().len(), n);
    // Gone from the file: its cache goes at the next refresh, and so do its events.
    write_links(&env, "# none\n");
    let r = refresh(&app, false, NOW + 3 * MIN).await;
    assert_eq!(r["links"], 0);
    assert!(app.cached_events().is_empty());
    assert_eq!(std::fs::read_dir(app.calendar_dir()).unwrap().count(), 0);
    assert_eq!(r["updatedAtMs"], Value::Null);
}

#[test]
fn a_second_refresh_while_one_runs_is_busy() {
    let env = Env::new();
    let app = test_app(&env);
    let _held = app.calendar.busy.lock().unwrap();
    assert_eq!(app.calendar_refresh_at(true, NOW)["busy"], true);
}

#[test]
fn a_hand_edited_interval_reads_as_the_default() {
    let env = Env::new();
    std::fs::write(
        env.data.path().join("settings.json"),
        r#"{"schema":1,"calendar":{"refreshMin":7,"x":1}}"#,
    )
    .unwrap();
    let app = env.app();
    let s = app.settings().0;
    assert_eq!(s.calendar.refresh_min, 30);
    assert_eq!(s.calendar.extra["x"], 1);
    let v: Value = serde_json::from_str(&app.get_settings_json()).unwrap();
    assert_eq!(v["settings"]["calendar"]["refreshMin"], 30);
}
