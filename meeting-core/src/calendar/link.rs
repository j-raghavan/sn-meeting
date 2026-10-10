//! F18-FR9 / FR11 / FR12: which event a recording belongs to, and what the meeting keeps of it.
//! The sidecar's `event` (schema 1, optional) holds what the minutes and Search need and never
//! the link URL (INV15). It lives in the sidecar's unknown-field map, so a hand-edited or newer
//! `event` is kept verbatim and never makes the sidecar unreadable.
use super::ics::{Attendee, Event};
use crate::sidecar::{Sidecar, TitleSource};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A recording may start this long before an event and still belong to it (F18-FR9).
pub const EARLY_MS: i64 = 5 * 60_000;
/// The sidecar key.
pub const FIELD: &str = "event";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkSource {
    /// Linked by time at Record (F18-FR9).
    Auto,
    /// Linked from the event sheet's "● Record", or by "Link to event…" (F18-FR10). The
    /// automatic rule never replaces it.
    User,
}

/// F18-FR12's `event`: `{uid, recurrenceId, seriesUid, linkId, calendar, start, end, title,
/// location, attendees, organizer, agenda, linkSource, alternatives}`, plus the event key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventLink {
    pub event_key: String,
    pub uid: String,
    #[serde(default)]
    pub recurrence_id: Option<String>,
    pub series_uid: String,
    pub link_id: String,
    #[serde(default)]
    pub calendar: Option<String>,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub organizer: Option<Attendee>,
    #[serde(default)]
    pub agenda: Option<String>,
    pub link_source: LinkSource,
    /// The other events that also qualified at Record (EC27), by key.
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl EventLink {
    /// What a meeting keeps of `e` (from the calendar named `calendar`).
    pub fn of(
        e: &Event,
        calendar: Option<&str>,
        source: LinkSource,
        alternatives: Vec<String>,
    ) -> EventLink {
        EventLink {
            event_key: e.event_key.clone(),
            uid: e.uid.clone(),
            recurrence_id: e.recurrence_id.clone(),
            series_uid: e.series_uid.clone(),
            link_id: e.event_key.split(':').next().unwrap_or_default().to_owned(),
            calendar: calendar.map(str::to_owned),
            start: e.start.clone(),
            end: e.end.clone(),
            title: e.title.clone(),
            location: e.location.clone(),
            attendees: e.attendees.clone(),
            organizer: e.organizer.clone(),
            agenda: e.agenda.clone(),
            link_source: source,
            alternatives,
            extra: Map::new(),
        }
    }

    pub fn start_ms(&self) -> Option<i64> {
        ms_of(&self.start)
    }
}

fn ms_of(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// The meeting's event, if it has a readable one.
pub fn event_of(sc: &Sidecar) -> Option<EventLink> {
    serde_json::from_value(sc.extra.get(FIELD)?.clone()).ok()
}

/// Link the meeting to `link` (F18-FR12): the event title becomes the meeting's title
/// (`titleSource: event`) unless the user named it. `None` unlinks (F18-FR10): the meeting is
/// unscheduled, and a title taken from the event goes with it.
pub fn set_event(sc: &mut Sidecar, link: Option<EventLink>) {
    match link {
        Some(l) => {
            let title = l.title.trim().to_owned();
            if sc.title_source != Some(TitleSource::User) && !title.is_empty() {
                sc.title = Some(title);
                sc.title_source = Some(TitleSource::Event);
            }
            if let Ok(v) = serde_json::to_value(&l) {
                sc.extra.insert(FIELD.into(), v);
            }
        }
        None => {
            sc.extra.remove(FIELD);
            if sc.title_source == Some(TitleSource::Event) {
                sc.title = None;
                sc.title_source = None;
            }
        }
    }
}

/// Whether `e` may be linked by time: not transparent, not all-day (F18-FR9); cancelled
/// instances never reach the cache.
pub fn linkable(e: &Event) -> bool {
    !e.transparent() && !e.all_day && e.status.as_deref() != Some("cancelled")
}

/// F18-FR9: the event a recording started at `t_ms` belongs to, and the others that also
/// qualified (EC27). An event qualifies from its start − 5 min to its end. The start nearest
/// the recording's wins; a tie goes to the shorter event, then to the first by title.
pub fn pick(events: &[&Event], t_ms: i64) -> Option<(usize, Vec<usize>)> {
    let mut q: Vec<usize> = (0..events.len())
        .filter(|&i| {
            let e = events[i];
            linkable(e) && e.start_ms - EARLY_MS <= t_ms && t_ms < e.end_ms
        })
        .collect();
    q.sort_by(|&a, &b| {
        let (x, y) = (events[a], events[b]);
        let key = |e: &Event| ((e.start_ms - t_ms).abs(), e.end_ms - e.start_ms);
        key(x).cmp(&key(y)).then_with(|| x.title.cmp(&y.title))
    });
    let first = *q.first()?;
    Some((first, q[1..].to_vec()))
}

/// The event "now" is inside (F18-FR7's boxed row): not all-day, `start <= now < end`; the
/// latest-started wins, then the shorter.
pub fn current(events: &[&Event], now_ms: i64) -> Option<usize> {
    (0..events.len())
        .filter(|&i| {
            let e = events[i];
            !e.all_day && e.start_ms <= now_ms && now_ms < e.end_ms
        })
        .min_by_key(|&i| {
            let e = events[i];
            (now_ms - e.start_ms, e.end_ms - e.start_ms)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: &str, title: &str, start: i64, end: i64) -> Event {
        Event {
            event_key: format!("aaaaaaaa:{key}:-"),
            uid: key.into(),
            recurrence_id: None,
            series_uid: key.into(),
            recurring: false,
            title: title.into(),
            start: String::new(),
            end: String::new(),
            start_ms: start,
            end_ms: end,
            all_day: false,
            location: None,
            attendees: vec![],
            organizer: None,
            agenda: None,
            status: None,
            transp: "opaque".into(),
            tz_unknown: false,
        }
    }

    const M: i64 = 60_000;

    #[test]
    fn ties_go_to_the_shorter_then_the_title() {
        let a = ev("a", "B", 0, 60 * M);
        let b = ev("b", "A", 0, 30 * M);
        let c = ev("c", "A", 0, 30 * M);
        let (w, alt) = pick(&[&a, &b, &c], 2 * M).unwrap();
        assert_eq!(w, 1, "shorter, then first by title (stable)");
        assert_eq!(alt, [2, 0]);
        // Before start − 5 min, at the end, transparent, all-day: none.
        assert!(pick(&[&a], -6 * M).is_none());
        assert!(pick(&[&a], -5 * M).is_some());
        assert!(pick(&[&a], 60 * M).is_none());
        let mut t = a.clone();
        t.transp = "transparent".into();
        let mut d = a.clone();
        d.all_day = true;
        let mut x = a.clone();
        x.status = Some("cancelled".into());
        assert!(pick(&[&t, &d, &x], M).is_none());
    }

    #[test]
    fn the_current_event() {
        let a = ev("a", "A", 0, 60 * M);
        let b = ev("b", "B", 30 * M, 60 * M);
        let mut d = ev("d", "D", 0, 24 * 60 * M);
        d.all_day = true;
        assert_eq!(current(&[&a, &b, &d], 10 * M), Some(0));
        assert_eq!(current(&[&a, &b, &d], 31 * M), Some(1));
        assert_eq!(current(&[&a, &b, &d], 60 * M), None);
    }

    #[test]
    fn linking_and_unlinking_the_sidecar() {
        let mut sc = crate::sidecar::blank("meeting-20261009-100000");
        let mut e = ev("u1", "Weekly sync", 0, M);
        e.start = "2026-10-09T10:00:00-07:00".into();
        let l = EventLink::of(&e, Some("Work"), LinkSource::Auto, vec!["k2".into()]);
        assert_eq!(l.link_id, "aaaaaaaa");
        assert_eq!(l.start_ms(), Some(1_791_565_200_000));
        set_event(&mut sc, Some(l.clone()));
        assert_eq!(sc.title.as_deref(), Some("Weekly sync"));
        assert_eq!(sc.title_source, Some(TitleSource::Event));
        assert_eq!(event_of(&sc), Some(l.clone()));
        assert_eq!(sc.extra["event"]["linkSource"], "auto");
        set_event(&mut sc, None);
        assert_eq!((sc.title.clone(), sc.title_source), (None, None));
        assert!(event_of(&sc).is_none());
        // A user's title stays, linked or not; an untitled event leaves the title alone.
        sc.title = Some("Mine".into());
        sc.title_source = Some(TitleSource::User);
        set_event(&mut sc, Some(l.clone()));
        set_event(&mut sc, None);
        assert_eq!(sc.title.as_deref(), Some("Mine"));
        let mut blank = crate::sidecar::blank("meeting-20261009-100000");
        let untitled = EventLink {
            title: " ".into(),
            ..l
        };
        set_event(&mut blank, Some(untitled));
        assert_eq!(blank.title, None);
        // A hand-edited `event` that isn't one is kept, and read as none.
        blank
            .extra
            .insert("event".into(), serde_json::json!("junk"));
        assert!(event_of(&blank).is_none());
    }
}
