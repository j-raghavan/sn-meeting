//! F18-FR10: "Link to event…" and "Unlink". A user link is `linkSource: user`, whatever the
//! times; unlinking makes the meeting unscheduled and drops a title taken from the event.
mod libcommon;

use libcommon::cal::*;
use libcommon::*;
use serde_json::Value;

fn v(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn link_and_unlink_by_hand() {
    let env = Env::new();
    let app = env.app();
    let review = ev("review", "Design review", TEN + 120 * MIN, TEN + 180 * MIN);
    calendar(&env, &app, vec![review.clone()]);
    // Recorded at 10:00, outside any event: unscheduled.
    let (id, dir) = record(&app, TEN);
    let day = |app: &meeting::app::App| {
        app.agenda_at("2026-10-09", "2026-10-09", TEN).unwrap()["days"][0]["rows"].clone()
    };
    assert_eq!(day(&app)[0]["unscheduled"], true);
    assert_eq!(
        v(&app.link_event_json(&id, &review.event_key)),
        serde_json::json!({"ok": true})
    );
    let sc = serde_json::to_value(read_sidecar(&dir, &id)).unwrap();
    assert_eq!(sc["event"]["linkSource"], "user");
    assert_eq!(sc["event"]["eventKey"], review.event_key.as_str());
    assert_eq!(sc["title"], "Design review");
    assert_eq!(sc["titleSource"], "event");
    let rows = day(&app);
    assert_eq!(
        rows.as_array().unwrap().len(),
        1,
        "folded into the event's row"
    );
    assert_eq!(rows[0]["meetingId"], id.as_str());
    // Unlink: unscheduled again, and the event's title goes with it.
    assert_eq!(
        v(&app.link_event_json(&id, "")),
        serde_json::json!({"ok": true})
    );
    let sc = serde_json::to_value(read_sidecar(&dir, &id)).unwrap();
    assert!(sc.get("event").is_none(), "{sc}");
    assert_eq!(sc["title"], Value::Null);
    assert_eq!(day(&app)[0]["unscheduled"], true);
    // A renamed meeting keeps its name through link and unlink.
    app.rename_json(&id, "Mine");
    app.link_event_json(&id, &review.event_key);
    app.link_event_json(&id, "");
    assert_eq!(read_sidecar(&dir, &id).title.as_deref(), Some("Mine"));
    // An event that has left the calendar, and a meeting that isn't there.
    assert!(v(&app.link_event_json(&id, "0123abcd:gone:-"))["error"].is_string());
    assert!(
        v(&app.link_event_json("meeting-20991231-000000", &review.event_key))["error"].is_string()
    );
}
