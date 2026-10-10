//! F20: the tag calls. Tags live in the sidecars (F20-FR3); every edit is a sidecar write
//! under the write lock, so the search index and the Tag Graph pick it up from the files.
use super::App;
use crate::api::error_json;
use crate::library::LibError;
use crate::sidecar::{self, Tag};
use crate::tags::{self, edit};
use serde_json::json;
use std::path::PathBuf;

/// The rename in progress, in the app's private folder: `{from, to}` (F20-FR4, resumable).
pub const RENAME_FILE: &str = "tag-rename.json";

fn tags_reply(r: Result<Vec<Tag>, LibError>) -> String {
    match r {
        Ok(t) => json!({ "tags": t }).to_string(),
        Err(e) => error_json(&e.to_string()),
    }
}

impl App {
    fn rename_path(&self) -> PathBuf {
        self.core.app_data_dir.join(RENAME_FILE)
    }

    /// `nativeAddTag(id, name)` (F20-FR4): the user's tag, normalised (F20-FR2).
    /// `{tags}`, or `{error}`.
    pub fn add_tag_json(&self, id: &str, name: &str) -> String {
        let Some(n) = tags::normalise(name) else {
            return error_json("A tag needs a letter or a digit");
        };
        let _w = self.write_lock();
        tags_reply(self.library().update(id, |sc, _| {
            edit::add(sc, &n);
            Ok(sc.tags.clone())
        }))
    }

    /// `nativeRemoveTag(id, name)` (F20-FR4): removed, and not brought back by a later summary.
    pub fn remove_tag_json(&self, id: &str, name: &str) -> String {
        let Some(n) = tags::normalise(name) else {
            return error_json("no such tag");
        };
        let _w = self.write_lock();
        tags_reply(self.library().update(id, |sc, _| {
            edit::remove(sc, &n);
            Ok(sc.tags.clone())
        }))
    }

    /// `nativeRenameTag(from, to)` (F20-FR4): `from` becomes `to` in every meeting in the save
    /// folders, merging where `to` is already there (F20-AC7). One sidecar at a time, each an
    /// atomic write; the rename is noted first, so one cut short is finished by the next call
    /// (or [`App::resume_tag_rename`]). `{renamed: n, to}`.
    pub fn rename_tag_json(&self, from: &str, to: &str) -> String {
        self.resume_tag_rename();
        let (Some(f), Some(t)) = (tags::normalise(from), tags::normalise(to)) else {
            return error_json("A tag needs a letter or a digit");
        };
        let note = json!({"from": f, "to": t}).to_string();
        if let Err(e) = crate::atomic::write_atomic(&self.rename_path(), note.as_bytes()) {
            return error_json(&format!("The rename can't start ({:?})", e.kind()));
        }
        let n = self.rename_everywhere(&f, &t);
        let _ = std::fs::remove_file(self.rename_path());
        json!({"renamed": n, "to": t}).to_string()
    }

    /// Finish a rename an earlier run didn't (the app was closed mid-way). How many meetings
    /// it changed, when there was one.
    pub fn resume_tag_rename(&self) -> Option<usize> {
        let bytes = crate::atomic::read_capped(&self.rename_path(), 4096).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let pair = (v["from"].as_str(), v["to"].as_str());
        let n = match pair {
            (Some(f), Some(t)) => match (tags::normalise(f), tags::normalise(t)) {
                (Some(f), Some(t)) => self.rename_everywhere(&f, &t),
                _ => 0,
            },
            _ => 0,
        };
        let _ = std::fs::remove_file(self.rename_path());
        Some(n)
    }

    /// The rename over every meeting that has `from`; a meeting without it isn't rewritten.
    fn rename_everywhere(&self, from: &str, to: &str) -> usize {
        let lib = self.library();
        let mut n = 0;
        for (id, f) in lib.scan() {
            if !f.has_sidecar {
                continue;
            }
            let _w = self.write_lock();
            let Ok(mut sc) = sidecar::read(&f.dir, &id) else {
                continue; // unreadable: never written over (F3-FR4)
            };
            if edit::rename(&mut sc, from, to) && sidecar::write(&f.dir, &sc).is_ok() {
                n += 1;
            }
        }
        n
    }
}
