//! F18: the calendar calls. Links are read from the user's file (F18-FR1) and stay in the core
//! (INV15); events are fetched into the app-private cache (F18-FR3/FR6) and read from it.
use super::App;
use crate::calendar::cache::{self, CacheFile, LinkStatus};
use crate::calendar::fetch::{self, Fetched, Validators};
use crate::calendar::ics::{self, Event};
use crate::calendar::link::{self, EventLink, LinkSource};
use crate::calendar::links::{self, Link, Links};
use crate::library::{MeetingSummary, TodoRef};
use crate::settings::Settings;
use crate::sidecar::Sidecar;
use serde_json::{Value, json};
use std::path::PathBuf;

/// A recording's first seconds are left alone (F18-FR4).
const RECORDING_GRACE_MS: i64 = 10_000;
const MINUTE_MS: i64 = 60_000;

/// One link with its cache: what Settings and Home show of it.
pub struct LinkState {
    pub link: Link,
    pub cache: CacheFile,
}

impl LinkState {
    /// F18-FR2's name: `X-WR-CALNAME`, else the label, else the host.
    pub fn name(&self) -> String {
        self.cache
            .calendar
            .clone()
            .or_else(|| self.link.label.clone())
            .unwrap_or_else(|| self.link.host.clone())
    }
}

impl App {
    /// The links file under the storage root.
    pub fn calendar_links_path(&self) -> PathBuf {
        links::path_in(&self.storage_root)
    }

    /// The links in the file, re-read when it changes (F18-FR1).
    pub fn calendar_links(&self) -> Links {
        self.calendar.links.get(&self.calendar_links_path())
    }

    /// Every link with its cache, in the file's order.
    pub fn link_states(&self) -> Vec<LinkState> {
        let dir = self.calendar_dir();
        self.calendar_links()
            .links
            .into_iter()
            .map(|link| LinkState {
                cache: cache::read(&dir, &link.id),
                link,
            })
            .collect()
    }

    /// Every link's secrets, for the scrubber (F18-FR2).
    pub fn link_secrets(&self) -> Vec<String> {
        self.calendar_links()
            .links
            .iter()
            .flat_map(Link::secrets)
            .collect()
    }

    /// The local day (`YYYY-MM-DD` → its midnight, epoch ms).
    pub(crate) fn day_start_ms(&self, day: chrono::NaiveDate) -> i64 {
        let midnight = day.and_hms_opt(0, 0, 0).expect("midnight");
        crate::tz::local_to_utc_ms(self.tz.as_ref(), midnight)
    }

    /// `nativeCalendarLinks`: `[{linkId, name, elided, label, lastOk, lastAttempt, status,
    /// reason, eventsThisWeek, skipped}]`. Never a URL (INV15).
    pub fn calendar_links_json(&self) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        json!(self.calendar_links_at(now)).to_string()
    }

    pub fn calendar_links_at(&self, now_ms: i64) -> Vec<Value> {
        let (from, to) = self.week_of(now_ms);
        self.link_states()
            .iter()
            .map(|s| {
                let c = &s.cache;
                let week = c
                    .events
                    .iter()
                    .filter(|e| e.start_ms < to && e.end_ms.max(e.start_ms + 1) > from)
                    .count();
                json!({
                    "linkId": s.link.id, "name": s.name(), "elided": s.link.elided,
                    "label": s.link.label,
                    "lastOk": (c.fetched_at_ms > 0).then_some(c.fetched_at_ms),
                    "lastAttempt": (c.attempted_at_ms > 0).then_some(c.attempted_at_ms),
                    "status": c.status, "reason": c.reason,
                    "eventsThisWeek": week, "skipped": c.skipped,
                })
            })
            .collect()
    }

    /// Monday 00:00 to the next Monday 00:00 of the week holding `now_ms`, local.
    pub(crate) fn week_of(&self, now_ms: i64) -> (i64, i64) {
        use chrono::Datelike;
        let today = crate::tz::local_at(self.tz.as_ref(), now_ms)
            .map(|t| t.date_naive())
            .unwrap_or_default();
        let monday = today - chrono::Days::new(u64::from(today.weekday().num_days_from_monday()));
        (
            self.day_start_ms(monday),
            self.day_start_ms(monday + chrono::Days::new(7)),
        )
    }

    /// `nativeCalendarRefresh(force)`: fetch the links that are due (F18-FR4). Blocking: one
    /// HTTPS GET per due link, each at most 30 s.
    pub fn calendar_refresh_json(&self, force: bool) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        self.calendar_refresh_at(force, now).to_string()
    }

    /// `calendar_refresh_json` at `now_ms`: `{updatedAtMs, failed: [linkId], fetched,
    /// links}`, `{busy: true}` while another refresh runs, or with `deferred` when an automatic
    /// refresh waits (a recording's first 10 s, or "manual only").
    pub fn calendar_refresh_at(&self, force: bool, now_ms: i64) -> Value {
        let Ok(_busy) = self.calendar.busy.try_lock() else {
            return json!({"busy": true});
        };
        let dir = self.calendar_dir();
        let links = self.calendar_links();
        let ids: Vec<&str> = links.links.iter().map(|l| l.id.as_str()).collect();
        cache::prune(&dir, &ids);
        let (s, _) = self.settings();
        let interval = i64::from(s.calendar.refresh_min) * MINUTE_MS;
        let deferred = if force {
            None
        } else if interval == 0 {
            Some("manual")
        } else if self.recording_started_within(now_ms, RECORDING_GRACE_MS) {
            Some("recording")
        } else {
            None
        };
        let mut fetched = 0;
        if deferred.is_none() {
            for link in &links.links {
                let c = cache::read(&dir, &link.id);
                let due = force || c.attempted_at_ms == 0 || now_ms - c.attempted_at_ms >= interval;
                if due {
                    let next = self.fetch_link(link, c, now_ms);
                    // A cache that can't be written keeps the last good one (F18-AC9).
                    let _ = cache::write(&dir, &next);
                    fetched += 1;
                }
            }
        }
        let mut out = self.refresh_summary();
        out["fetched"] = json!(fetched);
        if let Some(d) = deferred {
            out["deferred"] = json!(d);
        }
        out
    }

    /// "UPDATED 09:58 · 1 failed": the last successful refresh across all links and the links
    /// whose last attempt failed (F18-FR4).
    pub fn refresh_summary(&self) -> Value {
        let states = self.link_states();
        let updated = states
            .iter()
            .map(|s| s.cache.fetched_at_ms)
            .max()
            .unwrap_or(0);
        let failed: Vec<&str> = states
            .iter()
            .filter(|s| matches!(s.cache.status, LinkStatus::Failed | LinkStatus::Gone))
            .map(|s| s.link.id.as_str())
            .collect();
        json!({
            "updatedAtMs": (updated > 0).then_some(updated),
            "failed": failed,
            "links": states.len(),
        })
    }

    /// Whether the meeting being recorded began less than `ms` before `now_ms` (its id holds
    /// its local start, to the second).
    fn recording_started_within(&self, now_ms: i64, ms: i64) -> bool {
        let Some(id) = self.liveness().recording else {
            return false;
        };
        crate::library::stamp_of(&id)
            .map(|n| crate::tz::local_to_utc_ms(self.tz.as_ref(), n))
            .is_some_and(|start| now_ms - start < ms + 1000)
    }

    /// Fetch one link and fold the result into its cache. A failure keeps the last good events
    /// (F18-FR3, EC23); every reason is scrubbed of the link (F18-FR2).
    fn fetch_link(&self, link: &Link, mut c: CacheFile, now_ms: i64) -> CacheFile {
        // Validators only while the cached expansion is today's: the window moves daily.
        let same_day = |a: i64| {
            let d = |ms| crate::tz::local_at(self.tz.as_ref(), ms).map(|t| t.date_naive());
            a > 0 && d(a) == d(now_ms)
        };
        let v = if same_day(c.fetched_at_ms) {
            Validators {
                etag: c.etag.clone(),
                last_modified: c.last_modified.clone(),
            }
        } else {
            Validators::default()
        };
        let cfg = self.calendar.cfg;
        let url = test_url(link.expose(), &cfg);
        let r = crate::runtime::block_on(fetch::fetch(&url, &v, &cfg))
            .unwrap_or_else(|e| Fetched::Failed(format!("runtime: {e}")));
        c.attempted_at_ms = now_ms;
        let secrets = link.secrets();
        let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let fail = |c: &mut CacheFile, status, reason: &str| {
            c.status = status;
            c.reason = Some(crate::scrub::scrub(reason, &secrets));
        };
        match r {
            Fetched::Body { text, validators } => {
                let win = ics::window(now_ms, self.tz.as_ref());
                match ics::expand(&text, &link.id, self.tz.as_ref(), win) {
                    Some(x) => {
                        c.calendar = x.name;
                        c.events = x.events;
                        c.skipped = x.skipped;
                        c.etag = validators.etag;
                        c.last_modified = validators.last_modified;
                        c.status = LinkStatus::Ok;
                        c.reason = None;
                        c.fetched_at_ms = now_ms;
                    }
                    None => fail(&mut c, LinkStatus::Failed, "Not a calendar"),
                }
            }
            Fetched::NotModified => {
                c.status = LinkStatus::Ok;
                c.reason = None;
                c.fetched_at_ms = now_ms;
            }
            Fetched::Gone(code) => {
                fail(
                    &mut c,
                    LinkStatus::Gone,
                    &format!("{} ({code})", fetch::GONE_TEXT),
                );
            }
            Fetched::Failed(reason) => fail(&mut c, LinkStatus::Failed, &reason),
        }
        c
    }

    /// The cached event `key`, with its calendar's name.
    pub fn find_event(&self, key: &str) -> Option<(String, Event)> {
        let link_id = key.split(':').next()?;
        if !cache::is_link_id(link_id) {
            return None;
        }
        self.link_states()
            .into_iter()
            .find(|s| s.link.id == link_id)
            .and_then(|s| {
                let name = s.name();
                s.cache
                    .events
                    .into_iter()
                    .find(|e| e.event_key == key)
                    .map(|e| (name, e))
            })
    }

    /// F18-FR9: the event a recording started at `t_ms` belongs to, linked `auto`, with the
    /// other candidates as alternatives (EC27).
    pub fn auto_link(&self, t_ms: i64) -> Option<EventLink> {
        let events = self.cached_events();
        let refs: Vec<&Event> = events.iter().map(|(_, e)| e).collect();
        let (w, alt) = link::pick(&refs, t_ms)?;
        let alternatives = alt.iter().map(|&i| refs[i].event_key.clone()).collect();
        Some(EventLink::of(
            refs[w],
            Some(&events[w].0),
            LinkSource::Auto,
            alternatives,
        ))
    }

    /// The link "● Record" (F18-FR8) or "Link to event…" (F18-FR10) makes: `user`, whatever the
    /// times.
    pub fn user_link(&self, key: &str) -> Option<EventLink> {
        let (cal, e) = self.find_event(key)?;
        Some(EventLink::of(&e, Some(&cal), LinkSource::User, Vec::new()))
    }

    /// F18-FR11: the meetings linked to `e`'s series (its `seriesUid`, or a meeting the user
    /// linked into it) that started before this instance's link window, newest first. The
    /// meeting of this very instance is not "earlier".
    pub fn series_meetings(&self, e: &Event) -> Vec<(MeetingSummary, Sidecar)> {
        let before = e.start_ms - link::EARLY_MS;
        self.library()
            .all()
            .into_iter()
            .filter(|(m, sc)| {
                m.started_at_ms < before
                    && link::event_of(sc)
                        .is_some_and(|l| l.series_uid == e.series_uid && l.event_key != e.event_key)
            })
            .collect()
    }

    /// F18-FR11's "From last time": the open to-dos of the most recent earlier meeting in
    /// `e`'s series (ticking one ticks it in that meeting, F6-FR2).
    pub fn from_last_time(&self, e: &Event) -> Vec<TodoRef> {
        let Some((m, sc)) = self.series_meetings(e).into_iter().next() else {
            return Vec::new();
        };
        sc.todos
            .into_iter()
            .filter(|t| !t.done)
            .map(|t| TodoRef {
                meeting_id: m.id.clone(),
                meeting_started_at: m.started_at.clone(),
                meeting_title: sc.title.clone(),
                todo_id: t.id,
                text: t.text,
                owner: t.owner,
                due: t.due,
            })
            .collect()
    }

    /// F18-FR8: the template an event's title picks by the keyword rules (case-insensitive,
    /// first match, only a template a job could use), else Settings' default. The keyword that
    /// matched, if any.
    pub fn template_for(&self, title: &str) -> (String, Option<String>) {
        self.template_in(&self.settings().0, title)
    }

    /// [`App::template_for`] under settings `s` the caller already holds (Record holds the
    /// state lock).
    pub(crate) fn template_in(&self, s: &Settings, title: &str) -> (String, Option<String>) {
        let t = title.to_lowercase();
        s.calendar
            .template_rules
            .iter()
            .find(|r| {
                t.contains(&r.keyword.to_lowercase())
                    && self
                        .templates
                        .get(&r.template_id)
                        .is_some_and(|e| e.usable())
            })
            .map(|r| (r.template_id.clone(), Some(r.keyword.clone())))
            .unwrap_or_else(|| (self.effective_default(s), None))
    }

    /// `nativeEvent(eventKey)` (F18-FR8): the event sheet: the event, its calendar, the template
    /// its title picks, "Earlier in this series" (recurring only, newest first, F18-FR11), "From
    /// last time" and the meetings linked to this instance.
    pub fn event_json(&self, key: &str) -> String {
        let Some((cal, e)) = self.find_event(key) else {
            return crate::api::error_json("This event is no longer in the calendar");
        };
        let (tid, keyword) = self.template_for(&e.title);
        let series: Vec<Value> = if e.recurring {
            self.series_meetings(&e)
                .into_iter()
                .map(|(m, sc)| {
                    json!({"meetingId": m.id, "title": sc.title, "startedAt": m.started_at,
                           "openTodos": m.open_todo_count})
                })
                .collect()
        } else {
            Vec::new()
        };
        let linked: Vec<String> = self
            .library()
            .all()
            .into_iter()
            .filter(|(_, sc)| link::event_of(sc).is_some_and(|l| l.event_key == e.event_key))
            .map(|(m, _)| m.id)
            .collect();
        json!({
            "calendar": cal, "event": e,
            "template": {"id": tid, "keyword": keyword},
            "series": series, "fromLastTime": self.from_last_time(&e), "linked": linked,
        })
        .to_string()
    }

    /// `nativeLinkEvent(id, eventKey)` (F18-FR10): link the meeting to the event (`linkSource:
    /// user`, never replaced by the automatic rule), or unlink it with `""` (unscheduled).
    pub fn link_event_json(&self, id: &str, key: &str) -> String {
        let link = if key.is_empty() {
            None
        } else {
            match self.user_link(key) {
                Some(l) => Some(l),
                None => return crate::api::error_json("This event is no longer in the calendar"),
            }
        };
        let r = {
            let _w = self.write_lock();
            self.library().update(id, |sc, _| {
                link::set_event(sc, link);
                Ok(())
            })
        };
        self.edited(id, r)
    }

    /// Every cached event of the links in the file, with its link's name.
    pub fn cached_events(&self) -> Vec<(String, Event)> {
        self.link_states()
            .into_iter()
            .flat_map(|s| {
                let name = s.name();
                s.cache.events.into_iter().map(move |e| (name.clone(), e))
            })
            .collect()
    }
}

/// Tests only (`allow_http`, never on the device): a loopback link is fetched from the mock
/// server over `http:`; the links file accepts `https://` alone.
fn test_url(url: &str, cfg: &fetch::FetchCfg) -> String {
    match url.strip_prefix("https://") {
        Some(rest)
            if cfg.allow_http
                && (rest.starts_with("127.0.0.1:") || rest.starts_with("localhost:")) =>
        {
            format!("http://{rest}")
        }
        _ => url.to_owned(),
    }
}
