//! F19: the search calls. The index lives in app-private storage and is rebuilt from the
//! meeting files and the event cache alone (INV17); none of these calls a provider (F19-FR10).
use super::App;
use crate::api::error_json;
use crate::providers::{self, CallFail, ImageIn, JsonAsk};
use crate::search::Sources;
use crate::search::query::{self, Clock, Filter};
use crate::settings::Role;
use crate::spend::{Kind, UsageEntry, UsageRecord};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

/// F19-FR9's fixed instruction for a handwritten query.
pub const QUERY_SYSTEM: &str = "Return only the handwritten text, verbatim.";
/// A query's reply is a few words.
pub const QUERY_MAX_TOKENS: u64 = 64;
/// "Reading your writing…" lasts at most this long.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
/// The query strip: at most 1568 × 200 px, and a PNG of a few hundred KB at most.
pub const QUERY_MAX_W: u32 = 1568;
pub const QUERY_MAX_H: u32 = 200;
pub const QUERY_MAX_BYTES: usize = 2 * 1024 * 1024;
/// The longest recognised query kept.
pub const QUERY_MAX_CHARS: usize = 200;
/// Search usage records kept (oldest dropped first).
pub const MAX_QUERY_RECORDS: usize = 5_000;

/// A PNG's width and height from its IHDR chunk, when `b` is a PNG.
pub fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if b.len() < 24 || b[..8] != SIG || &b[12..16] != b"IHDR" {
        return None;
    }
    let n = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some((n(16), n(20)))
}

/// The query as the user meant it: the first line, unquoted, trimmed, capped.
fn clean_query(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line
        .trim_matches(|c| matches!(c, '"' | '\'' | '“' | '”' | '`'))
        .trim();
    line.chars()
        .filter(|c| !c.is_control())
        .take(QUERY_MAX_CHARS)
        .collect()
}

impl App {
    /// `<app data>/usage-query.json`: the usage of handwritten queries, which belong to no
    /// meeting (F19-FR9). App-private; not a cache, so not in the search folder.
    pub fn query_usage_path(&self) -> PathBuf {
        self.core.app_data_dir.join("usage-query.json")
    }

    /// The stored query usage; an unreadable file reads as none.
    pub fn query_usage(&self) -> Vec<UsageEntry> {
        crate::atomic::read_capped(&self.query_usage_path(), 16 * 1024 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn add_query_usage(&self, rec: UsageRecord) {
        let _w = self.write_lock();
        let mut all = self.query_usage();
        all.push(UsageEntry::from(rec));
        let excess = all.len().saturating_sub(MAX_QUERY_RECORDS);
        all.drain(..excess);
        if let Ok(bytes) = serde_json::to_vec(&all) {
            let _ = crate::atomic::write_atomic(&self.query_usage_path(), &bytes);
        }
    }

    /// `nativeRecogniseQuery(png)` (F19-FR9): the handwritten query strip read by the
    /// summarise provider, in at most 15 s. `{text}`, or `{error, offline}`; `offline` says
    /// there is no key, no network or no reply in time. Recorded as `kind: "query"`.
    pub fn recognise_query_json(&self, png: &[u8]) -> String {
        let bad = |e: &str| json!({"error": e, "offline": false}).to_string();
        match png_size(png) {
            None => return bad("not a PNG"),
            Some((w, h)) if w == 0 || h == 0 || w > QUERY_MAX_W || h > QUERY_MAX_H => {
                return bad("the query image is too large");
            }
            Some(_) if png.len() > QUERY_MAX_BYTES => return bad("the query image is too large"),
            Some(_) => {}
        }
        let (s, _) = self.settings();
        let (key, model) = match self.pick(&s, Role::Summarise) {
            Ok((k, m, _)) => (k, m),
            Err(e) => return json!({"error": e, "offline": true}).to_string(),
        };
        let budget = self.core.timeout.min(QUERY_TIMEOUT);
        let http = match providers::http_client(budget, self.core.https_only) {
            Ok(h) => h,
            Err(e) => return bad(&crate::scrub::scrub(&e, &[key.expose()])),
        };
        let ask = JsonAsk {
            system: QUERY_SYSTEM.into(),
            user: "Read the handwritten search query in this image.".into(),
            schema: serde_json::Value::Null,
            max_tokens: QUERY_MAX_TOKENS,
            images: vec![ImageIn {
                label: "The query:".into(),
                png: png.to_vec(),
            }],
        };
        let fut = providers::complete_text(&http, &self.core.endpoints, &key, &model, &ask);
        let r = match crate::runtime::block_on(
            async move { tokio::time::timeout(budget, fut).await },
        ) {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => return json!({"error": "Timed out", "offline": true}).to_string(),
            Err(e) => return error_json(&format!("runtime: {e}")),
        };
        let now = chrono::Utc::now().timestamp_millis();
        let at = crate::tz::local_at(self.tz.as_ref(), now)
            .map(|t| crate::tz::rfc3339(&t))
            .unwrap_or_default();
        let spent = match &r {
            Ok(x) => Some(&x.measured),
            Err(e) => e.spent.as_deref(),
        };
        if let Some(m) = spent {
            self.add_query_usage(UsageRecord::new(Kind::Query, m, at, &self.rates()));
        }
        match r {
            Ok(x) => json!({"text": clean_query(&x.text)}).to_string(),
            Err(e) => {
                let offline = matches!(
                    e.kind,
                    CallFail::NoConnection | CallFail::TimedOut | CallFail::KeyRejected
                );
                json!({"error": e.detail, "offline": offline}).to_string()
            }
        }
    }

    /// The event cache folder (F18-FR6): `<app data>/calendar`.
    pub fn calendar_dir(&self) -> PathBuf {
        self.core.app_data_dir.join("calendar")
    }

    fn with_sources<T>(&self, f: impl FnOnce(&Sources<'_>) -> T) -> T {
        let lib = self.library();
        f(&Sources {
            lib: &lib,
            calendar: self.calendar_dir(),
        })
    }

    /// Now, and the start of today in the device's time zone.
    pub fn clock(&self) -> Clock {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let midnight = self.today().and_hms_opt(0, 0, 0).unwrap_or_default();
        Clock {
            now_ms,
            today_start_ms: crate::tz::local_to_utc_ms(self.tz.as_ref(), midnight),
        }
    }

    /// `nativeSearch(query, filter, limit)` (F19-FR2): groups of hits from the local index
    /// only (F19-FR10). `limit` is the most groups; 0 or less means the default 50.
    pub fn search_json(&self, q: &str, filter: &str, limit: i32) -> String {
        self.search_at(q, filter, limit, self.clock())
    }

    /// `search_json` against `clock` (tests fix it).
    pub fn search_at(&self, q: &str, filter: &str, limit: i32, clock: Clock) -> String {
        let Some(f) = Filter::from_id(filter) else {
            return error_json("unknown filter");
        };
        let limit = usize::try_from(limit)
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(query::DEFAULT_GROUPS);
        let indexing = self.search.indexing();
        match self.search.with(|ix| query::search(ix, q, f, limit, clock)) {
            Ok(mut r) => {
                r.indexing = indexing;
                json!(r).to_string()
            }
            Err(_) => {
                self.search.reset();
                error_json("The search index can't be read; it is being rebuilt")
            }
        }
    }

    /// `nativeIndexStatus`: `{state: ready|indexing|missing, done, total}`.
    pub fn index_status_json(&self) -> String {
        json!(self.search.status()).to_string()
    }

    /// `nativeIndexUpdate` (F19-FR5): re-index the meetings and links whose files changed.
    /// Blocking; `{indexed, removed, rebuilt}`, or `{busy: true}` when one is already running.
    pub fn index_update_json(&self) -> String {
        match self.with_sources(|s| self.search.update(s)) {
            Some(u) => json!(u).to_string(),
            None => json!({"busy": true}).to_string(),
        }
    }

    /// `nativeRebuildIndex` (F19-FR5): start the index again and index everything. Blocking;
    /// progress through `nativeIndexStatus`.
    pub fn rebuild_index_json(&self) -> String {
        match self.with_sources(|s| self.search.rebuild(s)) {
            Some(u) => json!(u).to_string(),
            None => json!({"busy": true}).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::Core;
    use std::sync::Arc;

    /// EC29: an index that fails under a search is dropped and rebuilt by the next update.
    #[test]
    fn a_failing_index_is_reset() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut core = Core::device(data.path().to_str().unwrap());
        core.key_dir = data.path().join("no-keys");
        let tz = crate::tz::FixedTz(chrono::FixedOffset::east_opt(0).unwrap());
        let app = super::App::with_tz(core, root.path().into(), Arc::new(tz));
        app.index_update_json();
        assert!(app.search_json("x", "all", 0).contains("\"groups\":[]"));
        assert!(app.search_json("x", "nope", 0).contains("unknown filter"));
        app.search
            .with(|ix| ix.conn.execute_batch("DROP TABLE units"))
            .unwrap();
        let r = app.search_json("budget", "all", 10);
        assert!(r.contains("being rebuilt"), "{r}");
        assert!(!data.path().join("search/index.sqlite").exists());
        app.index_update_json();
        assert!(
            app.search_json("budget", "all", 10)
                .contains("\"groups\":[]")
        );
    }
}
