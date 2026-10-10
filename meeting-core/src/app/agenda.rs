//! F18-FR7 / FR14: `nativeAgenda`, the days Home shows: each day's events from the cache and
//! its meetings from the library, a meeting folded into the event it is linked to, the current
//! event with "From last time" (F18-FR11), and the marks (● recording, ✎ notes, ≡ minutes,
//! ☐n open to-dos). With no link set the days hold recordings alone (F18-FR14).
use super::App;
use crate::calendar::ics::Event;
use crate::calendar::link;
use crate::library::MeetingSummary;
use crate::sidecar::Sidecar;
use chrono::NaiveDate;
use serde_json::{Value, json};

/// The most days one call returns (a week strip is 7).
const MAX_DAYS: i64 = 42;
/// Attendees named on a row before "+n".
const PEOPLE_SHOWN: usize = 4;

/// A meeting's marks (F18-FR7): filled when it exists or is done.
pub fn marks(sc: Option<&Sidecar>, folder: Option<&str>) -> Value {
    let Some(sc) = sc else {
        return json!({"recording": false, "notes": false, "minutes": false, "todosOpen": 0, "todosTotal": 0,
                      "status": null, "audioLost": false});
    };
    let notes_dir = folder.map(|f| std::path::Path::new(f).join(format!("{}.notes", sc.id)));
    json!({
        "recording": !sc.segments.is_empty(),
        "notes": sc.extra.contains_key("notes") || notes_dir.is_some_and(|d| d.is_dir()),
        "minutes": sc.summary.as_deref().is_some_and(|s| !s.trim().is_empty()),
        "todosOpen": sc.todos.iter().filter(|t| !t.done).count(),
        "todosTotal": sc.todos.len(),
        // F18-FR7: the row says the meeting's state in words when it has nothing else.
        "status": sc.status,
        "audioLost": sc.error.as_deref() == Some(crate::library::AUDIO_LOST),
    })
}

/// "Priya Shah, Ana Ruiz +2": names (else addresses), one line.
pub fn people(e: &Event) -> String {
    let names: Vec<String> = e
        .attendees
        .iter()
        .filter_map(|a| a.name.clone().or_else(|| a.email.clone()))
        .collect();
    let mut s = names
        .iter()
        .take(PEOPLE_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > PEOPLE_SHOWN {
        s.push_str(&format!(" +{}", names.len() - PEOPLE_SHOWN));
    }
    s
}

impl App {
    /// `nativeAgenda(fromDay, toDay)`: `YYYY-MM-DD`, inclusive.
    pub fn agenda_json(&self, from: &str, to: &str) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        match self.agenda_at(from, to, now) {
            Ok(v) => v.to_string(),
            Err(e) => crate::api::error_json(e),
        }
    }

    /// The agenda at `now_ms`: `{hasLinks, today, nowMs, updatedAtMs, failed, days: [{day,
    /// dot, allDay: [row], rows: [row]}]}`.
    pub fn agenda_at(&self, from: &str, to: &str, now_ms: i64) -> Result<Value, &'static str> {
        let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
        let (Some(a), Some(b)) = (day(from), day(to)) else {
            return Err("days must be YYYY-MM-DD");
        };
        let n = (b - a).num_days();
        if !(0..MAX_DAYS).contains(&n) {
            return Err("toDay must be on or after fromDay, at most 6 weeks on");
        }
        let has_links = !self.calendar_links().links.is_empty();
        let events = self.cached_events();
        let meetings = self.library().all();
        let mut days = Vec::new();
        for i in 0..=n {
            let d = a + chrono::Days::new(i as u64);
            days.push(self.agenda_day(d, &events, &meetings, now_ms));
        }
        let today = crate::tz::local_at(self.tz.as_ref(), now_ms)
            .map(|t| t.date_naive().format("%Y-%m-%d").to_string());
        let summary = self.refresh_summary();
        Ok(json!({
            "hasLinks": has_links, "today": today, "nowMs": now_ms,
            "updatedAtMs": summary["updatedAtMs"], "failed": summary["failed"],
            "days": days,
        }))
    }

    fn agenda_day(
        &self,
        d: NaiveDate,
        events: &[(String, Event)],
        meetings: &[(MeetingSummary, Sidecar)],
        now_ms: i64,
    ) -> Value {
        let (ds, de) = (
            self.day_start_ms(d),
            self.day_start_ms(d + chrono::Days::new(1)),
        );
        let todays: Vec<&(String, Event)> = events
            .iter()
            .filter(|(_, e)| e.start_ms < de && (e.end_ms > ds || e.start_ms >= ds))
            .collect();
        let timed: Vec<&Event> = todays
            .iter()
            .map(|(_, e)| e)
            .filter(|e| !e.all_day)
            .collect();
        let current = (ds <= now_ms && now_ms < de)
            .then(|| link::current(&timed, now_ms))
            .flatten()
            .map(|i| timed[i].event_key.clone());
        let mine: Vec<&(MeetingSummary, Sidecar)> = meetings
            .iter()
            .filter(|(m, _)| ds <= m.started_at_ms && m.started_at_ms < de)
            .collect();
        let linked_to = |key: &str| -> Vec<&(MeetingSummary, Sidecar)> {
            mine.iter()
                .copied()
                .filter(|(_, sc)| link::event_of(sc).is_some_and(|l| l.event_key == key))
                .collect()
        };
        let mut all_day = Vec::new();
        let mut rows: Vec<(i64, Value)> = Vec::new();
        for (cal, e) in &todays {
            let linked = linked_to(&e.event_key);
            let newest = linked.first();
            let is_current = current.as_deref() == Some(e.event_key.as_str());
            let mut row = json!({
                "kind": "event", "eventKey": e.event_key, "title": e.title,
                "startMs": e.start_ms, "endMs": e.end_ms, "allDay": e.all_day,
                "location": e.location, "people": people(e), "calendar": cal,
                "recurring": e.recurring, "transparent": e.transparent(), "tzUnknown": e.tz_unknown,
                "current": is_current,
                "meetingId": newest.map(|(m, _)| m.id.clone()),
                "durationMs": newest.and_then(|(m, _)| m.duration_ms),
                "spend": newest.map(|(m, _)| m.spend.clone()),
                "meetings": linked.iter().map(|(m, _)| m.id.clone()).collect::<Vec<_>>(),
                "marks": marks(newest.map(|(_, sc)| sc), newest.map(|(m, _)| m.folder.as_str())),
            });
            if is_current {
                row["fromLastTime"] = json!(self.from_last_time(e));
            }
            if e.all_day {
                all_day.push(row);
            } else {
                rows.push((e.start_ms, row));
            }
        }
        let shown: std::collections::HashSet<&str> =
            todays.iter().map(|(_, e)| e.event_key.as_str()).collect();
        for (m, sc) in &mine {
            let ev = link::event_of(sc);
            if ev
                .as_ref()
                .is_some_and(|l| shown.contains(l.event_key.as_str()))
            {
                continue; // folded into its event's row
            }
            let end = m.started_at_ms + i64::try_from(m.duration_ms.unwrap_or(0)).unwrap_or(0);
            let title = sc.title.clone().or_else(|| {
                ev.as_ref()
                    .map(|l| l.title.clone())
                    .filter(|t| !t.is_empty())
            });
            rows.push((
                m.started_at_ms,
                json!({
                    "kind": "meeting", "meetingId": m.id, "title": title,
                    "startMs": m.started_at_ms, "endMs": end, "status": m.status,
                    "durationMs": m.duration_ms, "spend": m.spend,
                    "unscheduled": ev.is_none(),
                    "eventKey": ev.as_ref().map(|l| l.event_key.clone()),
                    "marks": marks(Some(sc), Some(&m.folder)),
                }),
            ));
        }
        rows.sort_by_key(|(t, _)| *t);
        let rows: Vec<Value> = rows.into_iter().map(|(_, r)| r).collect();
        json!({
            "day": d.format("%Y-%m-%d").to_string(),
            "dot": !rows.is_empty() || !all_day.is_empty(),
            "allDay": all_day, "rows": rows,
        })
    }
}
