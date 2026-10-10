//! F20: the tag calls. Tags live in the sidecars (F20-FR3); every edit is a sidecar write
//! under the write lock, so the search index and the Tag Graph pick it up from the files.
use super::App;
use crate::api::error_json;
use crate::library::LibError;
use crate::library::MeetingSummary;
use crate::sidecar::{self, Tag};
use crate::tags::graph::{self, Graph};
use crate::tags::{self, edit};
use chrono::NaiveDate;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// F20-FR9: the last layout and the tags it was made from (each meeting's, newest first).
#[derive(Debug, Default)]
pub struct GraphCache(Mutex<Option<(Vec<Vec<String>>, Graph)>>);

/// F20-FR7's filters: every recording, or those of the last three months.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    All,
    ThreeMonths,
}

impl Range {
    pub fn from_id(s: &str) -> Option<Range> {
        match s {
            "all" => Some(Range::All),
            "3m" => Some(Range::ThreeMonths),
            _ => None,
        }
    }
}

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

    /// The meetings in `range` (as of `today`) with their tags, newest first.
    fn tagged(&self, range: Range, today: NaiveDate) -> Vec<(MeetingSummary, Vec<String>)> {
        let since = match range {
            Range::All => i64::MIN,
            Range::ThreeMonths => today
                .checked_sub_months(chrono::Months::new(3))
                .map_or(i64::MIN, |d| self.day_start_ms(d)),
        };
        self.library()
            .all()
            .into_iter()
            .filter(|(m, _)| m.started_at_ms >= since)
            .map(|(m, sc)| (m, sc.tags.into_iter().map(|t| t.name).collect()))
            .collect()
    }

    /// `nativeTagGraph(range)` (F20-FR7 / FR9): `{range, graph: {nodes: [{name, count, x, y,
    /// r}], edges: [{a, b, shared, weak}], meetings, bounds}, laidOut}`, `range` `all` or `3m`.
    /// The layout runs (`laidOut`) only when the tags or counts changed since the last call.
    pub fn tag_graph_json(&self, range: &str) -> String {
        self.tag_graph_at(range, self.today())
    }

    /// `tag_graph_json` as of `today` (tests fix it).
    pub fn tag_graph_at(&self, range: &str, today: NaiveDate) -> String {
        let Some(r) = Range::from_id(range) else {
            return error_json("range must be all or 3m");
        };
        self.resume_tag_rename();
        let input: Vec<Vec<String>> = self
            .tagged(r, today)
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| !t.is_empty())
            .collect();
        let mut cache = self
            .tag_graph
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (g, laid_out) = match cache.as_ref() {
            Some((seen, g)) if *seen == input => (g.clone(), false),
            _ => {
                let g = graph::build(&input);
                *cache = Some((input, g.clone()));
                (g, true)
            }
        };
        json!({"range": range, "graph": g, "laidOut": laid_out}).to_string()
    }

    /// `nativeTagMeetings(name, range)` (F20-FR8): the recordings tagged `name`, newest first
    /// (by `startedAt`): `{tag, count, meetings: [{id, title, startedAt, startedAtMs,
    /// durationMs, tags}]}`, `tags` being the meeting's other tags.
    pub fn tag_meetings_json(&self, name: &str, range: &str) -> String {
        self.tag_meetings_at(name, range, self.today())
    }

    /// `tag_meetings_json` as of `today` (tests fix it).
    pub fn tag_meetings_at(&self, name: &str, range: &str, today: NaiveDate) -> String {
        let Some(r) = Range::from_id(range) else {
            return error_json("range must be all or 3m");
        };
        let Some(tag) = tags::normalise(name) else {
            return error_json("no such tag");
        };
        // `tagged` is newest first already (F3-FR3's order: startedAt, then id).
        let rows: Vec<serde_json::Value> = self
            .tagged(r, today)
            .into_iter()
            .filter(|(_, t)| t.contains(&tag))
            .map(|(m, t)| {
                let others: Vec<&String> = t.iter().filter(|n| **n != tag).collect();
                json!({
                    "id": m.id, "title": m.title, "startedAt": m.started_at,
                    "startedAtMs": m.started_at_ms, "durationMs": m.duration_ms, "tags": others,
                })
            })
            .collect();
        json!({"tag": tag, "count": rows.len(), "meetings": rows}).to_string()
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
