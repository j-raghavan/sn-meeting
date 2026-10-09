//! F18-FR2 / F18-AC3 / INV15: a calendar link is a secret. With fixture links fetched from a
//! mock server (successes, a 404, a redirect loop, junk), every reply the shell can get, the
//! sidecars, settings.json, the event cache and the search index are searched for the links'
//! URLs, paths and queries: none appear. Only the id, the name and the elided form leave.
mod libcommon;

use libcommon::*;
use meeting::calendar::fetch::FetchCfg;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "private-LEAKTOKEN9876543210";
const QUERY: &str = "auth=QSECRETQSECRETQ";
/// 2026-10-07 12:00 PDT, in the fixture's week.
const NOW: i64 = 1_791_399_600_000;

fn needles(s: &MockServer) -> Vec<String> {
    let port = s.address().port();
    let p1 = format!("/calendar/ical/leak%40example.com/{TOKEN}/basic.ics");
    vec![
        TOKEN.to_owned(),
        QUERY.to_owned(),
        "QSECRETQSECRETQ".to_owned(),
        p1.clone(),
        format!("https://127.0.0.1:{port}{p1}"),
        format!("http://127.0.0.1:{port}{p1}"),
        "/owa/calendar/LEAKOUTLOOK123456/reachcalendar.ics".to_owned(),
        "LEAKOUTLOOK123456".to_owned(),
    ]
}

fn assert_clean(what: &str, text: &str, needles: &[String]) {
    for n in needles {
        assert!(!text.contains(n.as_str()), "{what} holds {n}: {text}");
    }
}

/// Every file under `dir`, as text (lossy), with its path.
fn files(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if let Ok(b) = std::fs::read(&p) {
            out.push((
                p.display().to_string(),
                String::from_utf8_lossy(&b).into_owned(),
            ));
        }
    }
}

#[tokio::test]
async fn no_link_url_leaves_the_core() {
    let env = Env::new();
    let mut app = env.app();
    app.calendar.cfg = FetchCfg {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(3),
        allow_http: true,
        ..FetchCfg::default()
    };
    let app = Arc::new(app);
    let s = MockServer::start().await;
    let port = s.address().port();
    let ics = std::fs::read_to_string(format!(
        "{}/tests/fixtures/calendar/google.ics",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    Mock::given(path(format!(
        "/calendar/ical/leak%40example.com/{TOKEN}/basic.ics"
    )))
    .respond_with(ResponseTemplate::new(200).set_body_string(ics))
    .mount(&s)
    .await;
    Mock::given(path("/owa/calendar/LEAKOUTLOOK123456/reachcalendar.ics"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            "/owa/calendar/LEAKOUTLOOK123456/reachcalendar.ics?again=LEAKOUTLOOK123456",
        ))
        .mount(&s)
        .await;
    let links = format!(
        "Work = https://127.0.0.1:{port}/calendar/ical/leak%40example.com/{TOKEN}/basic.ics\n\
         https://127.0.0.1:{port}/owa/calendar/LEAKOUTLOOK123456/reachcalendar.ics?{QUERY}\n\
         https://127.0.0.1:{port}/gone/{TOKEN}/x.ics\n\
         http://insecure.example/{TOKEN}\n"
    );
    let lf = env.root.path().join("MyStyle/Meetings/calendar-links.txt");
    std::fs::create_dir_all(lf.parent().unwrap()).unwrap();
    std::fs::write(&lf, links).unwrap();
    let n = needles(&s);

    let a2 = app.clone();
    let r = tokio::task::spawn_blocking(move || a2.calendar_refresh_at(true, NOW).to_string())
        .await
        .unwrap();
    assert_clean("nativeCalendarRefresh", &r, &n);
    let mut replies = vec![("nativeCalendarRefresh", r)];
    let listed = app.calendar_links_json();
    assert!(listed.contains("https://127.0.0.1/…"), "elided: {listed}");
    assert!(
        listed.contains("Too many redirects") && listed.contains("no longer works"),
        "{listed}"
    );
    replies.push(("nativeCalendarLinks", listed));
    // Record during the fixture's Monday stand-up (linked by time), and one from the sheet.
    let key = app
        .cached_events()
        .into_iter()
        .find(|(_, e)| e.title == "Team stand-up")
        .unwrap()
        .1
        .event_key;
    let b = app.begin_json(
        1_791_219_660_000,
        &mic_json().to_string(),
        &audio_json().to_string(),
    );
    let id = serde_json::from_str::<serde_json::Value>(&b).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    replies.push(("nativeBeginMeeting", b));
    replies.push(("nativeFinishMeeting", app.finish_json(&id, 1000, "")));
    replies.push((
        "nativeBeginMeetingForEvent",
        app.begin_linked(
            NOW,
            &mic_json().to_string(),
            &audio_json().to_string(),
            Some(&key),
        ),
    ));
    replies.push(("nativeEvent", app.event_json(&key)));
    replies.push(("nativeAgenda", app.agenda_json("2026-10-05", "2026-10-11")));
    replies.push(("nativeLinkEvent", app.link_event_json(&id, &key)));
    replies.push(("nativeGetMeeting", app.get_meeting_json(&id)));
    replies.push(("nativeListRecent", app.list_recent_json(10)));
    replies.push(("nativeOpenTodos", app.open_todos_json()));
    replies.push(("nativeGetSettings", app.get_settings_json()));
    // A link pasted into Settings is refused without being echoed.
    let pasted = serde_json::json!({"calendar": {"templateRules": [{"keyword": format!("/calendar/ical/leak%40example.com/{TOKEN}/basic.ics"), "templateId": "general"}]}});
    replies.push((
        "nativeSetSettings",
        app.set_settings_json(&pasted.to_string()),
    ));
    let a3 = app.clone();
    replies.push((
        "nativeIndexUpdate",
        tokio::task::spawn_blocking(move || a3.index_update_json())
            .await
            .unwrap(),
    ));
    replies.push(("nativeSearch", app.search_json("stand-up", "all", 0)));
    for (name, r) in &replies {
        assert_clean(name, r, &n);
    }
    // The files: sidecars and transcripts in the save folder, settings.json, the event cache
    // and the search index in app data.
    let mut all = Vec::new();
    files(env.root.path(), &mut all);
    files(env.data.path(), &mut all);
    let mut seen = 0;
    for (p, text) in &all {
        if p.ends_with("calendar-links.txt") {
            continue; // the user's own file, read-only to Meetings (INV16)
        }
        assert_clean(p, text, &n);
        seen += 1;
    }
    assert!(seen >= 4, "{all:?}");
    // The links file is never written.
    assert!(std::fs::read_to_string(&lf).unwrap().contains(TOKEN));
}

#[test]
fn scrubbing_removes_a_link_and_its_path_from_any_message() {
    let l = meeting::calendar::links::parse(&format!(
        "https://calendar.google.com/calendar/ical/x/{TOKEN}/basic.ics?{QUERY}"
    ));
    let link = &l.links[0];
    let secrets = link.secrets();
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
    let msg = format!(
        "error sending request for url ({}): reset; path {}",
        link.expose(),
        &link.expose()[27..]
    );
    let out = meeting::scrub::scrub(&msg, &secrets);
    assert!(!out.contains(TOKEN) && !out.contains("QSECRETQ"), "{out}");
}
