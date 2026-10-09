//! F19: the search calls. The index lives in app-private storage and is rebuilt from the
//! meeting files and the event cache alone (INV17); none of these calls a provider (F19-FR10).
use super::App;
use crate::search::Sources;
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
