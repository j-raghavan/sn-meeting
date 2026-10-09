//! F18-FR5: one feed's text to the events in the window. Parsing and recurrence are calcard's
//! (D18): `expand_dates` applies `RRULE`, `EXDATE`, `RDATE` and `RECURRENCE-ID` overrides, and
//! `TZID`s resolve through IANA names, Outlook's Windows names and MS CDO ids. Each series
//! (one `UID`) is expanded on its own, so a long-running or hostile series can use only its
//! own budget (EC25). Floating times, and times in a zone nothing resolves (EC26), are read as
//! device local time.
use crate::tz::{self, LocalTz};
use calcard::common::timezone::Tz;
use calcard::icalendar::dates::TimeOrDelta;
use calcard::icalendar::{
    ICalendar, ICalendarComponent, ICalendarComponentType, ICalendarEntry, ICalendarParameterName,
    ICalendarProperty, ICalendarValue,
};
use chrono::{DateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// F18-FR5's window around today, in days.
pub const PAST_DAYS: i64 = 90;
pub const FUTURE_DAYS: i64 = 60;
/// At most this many instances of one series are kept (F18-FR5).
pub const MAX_PER_SERIES: usize = 2_000;
/// The most instances one series may generate on the way to the window (a daily meeting since
/// 1970 is about 20 500).
pub const SERIES_BUDGET: usize = 40_000;
/// The most instances a whole feed may generate (EC25).
pub const FEED_BUDGET: usize = 200_000;
/// An event's description is kept to this many bytes (F18-FR6).
pub const MAX_AGENDA: usize = 8 * 1024;
const MAX_TEXT: usize = 500;
const MAX_ATTENDEES: usize = 100;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// One instance after expansion (F18-FR6's fields). `agenda` is the event's `DESCRIPTION` and
/// `transp` its `TRANSP`, under the names Search reads (M14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// `<linkId>:<uid>:<recurrenceId|->`; the recurrence part is compact UTC
    /// (`20261009T170000Z`, or `20261009` for an all-day one), so it has no `:`.
    pub event_key: String,
    pub uid: String,
    /// The instance's original start (RFC 3339, device local), for a recurring series.
    #[serde(default)]
    pub recurrence_id: Option<String>,
    /// The series: the `UID` without `RECURRENCE-ID` (F18-FR11).
    pub series_uid: String,
    #[serde(default)]
    pub recurring: bool,
    #[serde(default)]
    pub title: String,
    /// RFC 3339, device local.
    pub start: String,
    pub end: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub organizer: Option<Attendee>,
    #[serde(default)]
    pub agenda: Option<String>,
    /// `STATUS`, lowercased (`confirmed`, `tentative`); cancelled instances are dropped.
    #[serde(default)]
    pub status: Option<String>,
    /// `opaque` or `transparent`.
    #[serde(default = "opaque")]
    pub transp: String,
    /// EC26: its `TZID` resolved to nothing, so it was read as device local time.
    #[serde(default)]
    pub tz_unknown: bool,
}

fn opaque() -> String {
    "opaque".into()
}

impl Event {
    pub fn transparent(&self) -> bool {
        self.transp == "transparent"
    }
}

/// A feed, expanded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expanded {
    /// `X-WR-CALNAME`.
    pub name: Option<String>,
    pub events: Vec<Event>,
    /// Events that couldn't be read ("2 events couldn't be read", F18-FR5).
    pub skipped: usize,
}

/// `[from_ms, to_ms)`: F18-FR5's window around `now_ms`, from local midnight.
pub fn window(now_ms: i64, tz: &dyn LocalTz) -> (i64, i64) {
    let day = tz::local_at(tz, now_ms).map_or(now_ms, |t| {
        let midnight = t.date_naive().and_hms_opt(0, 0, 0).expect("midnight");
        tz::local_to_utc_ms(tz, midnight)
    });
    (day - PAST_DAYS * DAY_MS, day + (FUTURE_DAYS + 1) * DAY_MS)
}

/// F18-FR5: parse `text` and expand every event overlapping `win`. `None` when it isn't a
/// calendar at all.
pub fn expand(text: &str, link_id: &str, tz: &dyn LocalTz, win: (i64, i64)) -> Option<Expanded> {
    let ical = ICalendar::parse(text).ok()?;
    let mut out = Expanded {
        name: ical
            .components
            .iter()
            .find(|c| c.component_type == ICalendarComponentType::VCalendar)
            .and_then(|c| other_text(c, "X-WR-CALNAME"))
            .map(|n| clip(&n, MAX_TEXT))
            .filter(|n| !n.is_empty()),
        ..Expanded::default()
    };
    let zones: Vec<ICalendarComponent> = ical.timezones().cloned().collect();
    // Series in the order the feed first names them.
    let mut order: Vec<String> = Vec::new();
    let mut series: HashMap<String, Vec<&ICalendarComponent>> = HashMap::new();
    for c in &ical.components {
        if c.component_type != ICalendarComponentType::VEvent {
            continue;
        }
        let Some(uid) = c.uid().map(str::trim).filter(|u| !u.is_empty()) else {
            out.skipped += 1;
            continue;
        };
        let uid = clip(uid, MAX_TEXT);
        if !series.contains_key(&uid) {
            order.push(uid.clone());
        }
        series.entry(uid).or_default().push(c);
    }
    let mut budget = FEED_BUDGET;
    for uid in order {
        let comps = &series[&uid];
        if budget == 0 {
            out.skipped += comps.len();
            continue;
        }
        let limit = budget.min(SERIES_BUDGET);
        let (mut evs, generated, bad) = one_series(&uid, comps, &zones, link_id, tz, win, limit);
        budget = budget.saturating_sub(generated);
        out.skipped += bad;
        evs.sort_by_key(|e| (e.start_ms, e.end_ms));
        evs.truncate(MAX_PER_SERIES);
        out.events.extend(evs);
    }
    let mut seen = std::collections::HashSet::new();
    out.events.retain(|e| seen.insert(e.event_key.clone()));
    out.events
        .sort_by(|a, b| (a.start_ms, a.end_ms, &a.title).cmp(&(b.start_ms, b.end_ms, &b.title)));
    Some(out)
}

/// One series' instances in the window, how many calcard generated, and how many of its
/// components it couldn't read.
fn one_series(
    uid: &str,
    comps: &[&ICalendarComponent],
    zones: &[ICalendarComponent],
    link_id: &str,
    tz: &dyn LocalTz,
    win: (i64, i64),
    limit: usize,
) -> (Vec<Event>, usize, usize) {
    let mut components = zones.to_vec();
    // calcard matches an override to its instance by (SEQUENCE, RECURRENCE-ID); Google bumps
    // an override's SEQUENCE past the master's, which would leave the moved instance in place.
    // Within one UID the overrides belong to the series whatever their sequence.
    components.extend(comps.iter().map(|c| {
        let mut c = (*c).clone();
        c.entries.retain(|e| e.name != ICalendarProperty::Sequence);
        c
    }));
    let sub = ICalendar { components };
    let resolver = sub.build_tz_resolver();
    let x = sub.expand_dates(Tz::Floating, limit);
    let generated = x.events.len();
    let mut bad: std::collections::HashSet<u32> = x.errors.iter().map(|e| e.comp_id).collect();
    let recurring = comps.iter().any(|c| {
        c.has_property(&ICalendarProperty::Rrule)
            || c.has_property(&ICalendarProperty::Rdate)
            || c.has_property(&ICalendarProperty::RecurrenceId)
    });
    let mut evs = Vec::new();
    for ce in x.events {
        let Some(comp) = sub
            .components
            .get(ce.comp_id as usize)
            .filter(|c| c.component_type == ICalendarComponentType::VEvent)
        else {
            continue;
        };
        let start_ms = ms(&ce.start, tz);
        let end_ms = match &ce.end {
            TimeOrDelta::Time(t) => ms(t, tz),
            TimeOrDelta::Delta(d) => start_ms.saturating_add(d.num_milliseconds()),
        }
        .max(start_ms);
        if status(comp).as_deref() == Some("cancelled") {
            continue;
        }
        let overlaps = end_ms > win.0 && start_ms < win.1
            || (end_ms == start_ms && start_ms >= win.0 && start_ms < win.1);
        if !overlaps {
            continue;
        }
        let all_day = dtstart(comp).is_some_and(|(dt, _)| !dt.has_time());
        let tz_unknown = dtstart(comp)
            .and_then(|(_, e)| e.tz_id())
            .is_some_and(|id| resolver.resolve(id).is_none());
        let rid_ms = recurring.then(|| {
            rid_of(comp, &resolver, tz)
                .filter(|_| own_start(comp, &resolver, tz) == Some(start_ms))
                .unwrap_or(start_ms)
        });
        let key_part = match rid_ms {
            None => "-".to_owned(),
            Some(r) => compact(r, all_day, tz),
        };
        match event_of(
            comp,
            uid,
            link_id,
            &key_part,
            rid_ms,
            recurring,
            (start_ms, end_ms),
            all_day,
            tz_unknown,
            tz,
        ) {
            Some(e) => evs.push(e),
            None => {
                bad.insert(ce.comp_id);
            }
        }
    }
    (evs, generated, bad.len())
}

#[allow(clippy::too_many_arguments)]
fn event_of(
    comp: &ICalendarComponent,
    uid: &str,
    link_id: &str,
    key_part: &str,
    rid_ms: Option<i64>,
    recurring: bool,
    (start_ms, end_ms): (i64, i64),
    all_day: bool,
    tz_unknown: bool,
    tz: &dyn LocalTz,
) -> Option<Event> {
    let stamp = |ms| tz::local_at(tz, ms).map(|t| tz::rfc3339(&t));
    Some(Event {
        event_key: format!("{link_id}:{uid}:{key_part}"),
        uid: uid.to_owned(),
        recurrence_id: match rid_ms {
            Some(r) => Some(stamp(r)?),
            None => None,
        },
        series_uid: uid.to_owned(),
        recurring,
        title: text(comp, &ICalendarProperty::Summary)
            .map(|t| one_line(&clip(&t, MAX_TEXT)))
            .unwrap_or_default(),
        start: stamp(start_ms)?,
        end: stamp(end_ms)?,
        start_ms,
        end_ms,
        all_day,
        location: text(comp, &ICalendarProperty::Location)
            .map(|t| one_line(&clip(&t, MAX_TEXT)))
            .filter(|t| !t.is_empty()),
        attendees: comp
            .properties(&ICalendarProperty::Attendee)
            .take(MAX_ATTENDEES)
            .filter_map(person)
            .collect(),
        organizer: comp
            .property(&ICalendarProperty::Organizer)
            .and_then(person),
        agenda: text(comp, &ICalendarProperty::Description)
            .map(|t| clip_bytes(t.trim(), MAX_AGENDA))
            .filter(|t| !t.is_empty()),
        status: status(comp),
        transp: match text(comp, &ICalendarProperty::Transp) {
            Some(t) if t.eq_ignore_ascii_case("transparent") => "transparent".into(),
            _ => opaque(),
        },
        tz_unknown,
    })
}

/// An `ATTENDEE` or `ORGANIZER`: its `CN` and its `mailto:` address.
fn person(e: &ICalendarEntry) -> Option<Attendee> {
    let name = e
        .parameters(&ICalendarParameterName::Cn)
        .find_map(|v| v.as_text())
        .map(|n| one_line(&clip(n.trim().trim_matches('"'), MAX_TEXT)))
        .filter(|n| !n.is_empty());
    let email = e
        .values
        .first()
        .and_then(|v| v.as_text())
        .map(|v| {
            let v = v.trim();
            match v.get(..7) {
                Some(p) if p.eq_ignore_ascii_case("mailto:") => &v[7..],
                _ => v,
            }
        })
        .map(|v| clip(v, MAX_TEXT))
        .filter(|v| v.contains('@'));
    (name.is_some() || email.is_some()).then_some(Attendee { name, email })
}

fn text(c: &ICalendarComponent, p: &ICalendarProperty) -> Option<String> {
    c.property(p)
        .and_then(|e| e.values.first())
        .and_then(ICalendarValue::as_text)
        .map(str::to_owned)
}

fn other_text(c: &ICalendarComponent, name: &str) -> Option<String> {
    c.entries
        .iter()
        .find(|e| matches!(&e.name, ICalendarProperty::Other(n) if n.eq_ignore_ascii_case(name)))
        .and_then(|e| e.values.first())
        .and_then(ICalendarValue::as_text)
        .map(|s| one_line(s.trim()))
}

fn status(c: &ICalendarComponent) -> Option<String> {
    text(c, &ICalendarProperty::Status).map(|s| s.to_ascii_lowercase())
}

fn dtstart(c: &ICalendarComponent) -> Option<(&calcard::common::PartialDateTime, &ICalendarEntry)> {
    let e = c.property(&ICalendarProperty::Dtstart)?;
    match e.values.first()? {
        ICalendarValue::PartialDateTime(dt) => Some((dt, e)),
        _ => None,
    }
}

/// A date-time property of `c` as epoch ms, in its `TZID` (or as written, or device local).
fn prop_ms(
    c: &ICalendarComponent,
    p: &ICalendarProperty,
    r: &calcard::icalendar::timezone::TzResolver<&str>,
    tz: &dyn LocalTz,
) -> Option<i64> {
    let e = c.property(p)?;
    let ICalendarValue::PartialDateTime(dt) = e.values.first()? else {
        return None;
    };
    let t = dt.to_date_time_with_tz(r.resolve_or_default(e.tz_id()))?;
    Some(ms(&t, tz))
}

fn rid_of(
    c: &ICalendarComponent,
    r: &calcard::icalendar::timezone::TzResolver<&str>,
    tz: &dyn LocalTz,
) -> Option<i64> {
    prop_ms(c, &ICalendarProperty::RecurrenceId, r, tz)
}

fn own_start(
    c: &ICalendarComponent,
    r: &calcard::icalendar::timezone::TzResolver<&str>,
    tz: &dyn LocalTz,
) -> Option<i64> {
    prop_ms(c, &ICalendarProperty::Dtstart, r, tz)
}

/// Epoch ms of a calcard time; a floating one (or an unresolved zone) is device local time.
fn ms(t: &DateTime<Tz>, tz: &dyn LocalTz) -> i64 {
    if t.timezone().is_floating() {
        tz::local_to_utc_ms(tz, t.naive_local())
    } else {
        t.timestamp_millis()
    }
}

/// The key's recurrence part: `20261009T170000Z`, or the local date for an all-day instance.
fn compact(ms: i64, all_day: bool, tz: &dyn LocalTz) -> String {
    if all_day {
        return tz::local_at(tz, ms)
            .map_or_else(|| ms.to_string(), |t| t.format("%Y%m%d").to_string());
    }
    chrono::Utc.timestamp_millis_opt(ms).single().map_or_else(
        || ms.to_string(),
        |t| t.format("%Y%m%dT%H%M%SZ").to_string(),
    )
}

fn clip(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// At most `max` bytes, cut on a character boundary.
fn clip_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_characters_whole() {
        assert_eq!(clip_bytes("ééé", 3), "é");
        assert_eq!(clip_bytes("abc", 8), "abc");
        assert_eq!(one_line(" a\nb\t"), "a b");
        assert_eq!(clip("abcdef", 2), "ab");
    }

    #[test]
    fn junk_is_not_a_calendar() {
        let tz = crate::tz::FixedTz(chrono::FixedOffset::east_opt(0).unwrap());
        assert!(expand("hello", "l", &tz, (0, 1)).is_none());
    }
}
