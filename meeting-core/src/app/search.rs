//! F19: the search calls. The index lives in app-private storage and is rebuilt from the
//! meeting files and the event cache alone (INV17); none of these calls a provider (F19-FR10).
use super::App;
use crate::api::error_json;
use crate::search::Sources;
use crate::search::query::{self, Clock, Filter};
use serde_json::json;
use std::path::PathBuf;

impl App {
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
