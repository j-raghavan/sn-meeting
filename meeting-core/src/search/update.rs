//! F19-FR5: keeping the index current. Each group remembers what its files looked like (each
//! file's modification time and size); an update re-indexes only the groups whose files
//! changed, adds new ones and drops those whose files are gone (D5). The index is rebuilt from
//! nothing when it is missing, unreadable or of an older version, and on request. Nothing here
//! calls a provider (F19-FR10, INV17).
use super::index::{Group, GroupKind, Index, key_of};
use super::units::{self, EventDoc};
use crate::library::Library;
use crate::notes_text;
use crate::sidecar;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The index file inside the search folder (`<filesDir>/search/index.*`).
pub const FILE: &str = "index.sqlite";
/// An event cache file is small; bigger is not ours.
const MAX_EVENT_FILE: u64 = 8 * 1024 * 1024;

/// `nativeIndexStatus`: `{state: ready|indexing|missing, done, total}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub state: &'static str,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Default)]
struct Progress {
    done: usize,
    total: usize,
    /// No update has finished on this index since it was created.
    empty: bool,
}

/// What an update did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Updated {
    pub indexed: usize,
    pub removed: usize,
    /// The index was started again from nothing first.
    pub rebuilt: bool,
}

/// The index and its upkeep, owned by `App`. Searches and updates share it; an update holds
/// the index only while it writes one group, so a search during a rebuild sees what is indexed.
#[derive(Debug)]
pub struct Searcher {
    /// `<filesDir>/search`; `None` keeps the index in memory.
    dir: Option<PathBuf>,
    ix: Mutex<Option<Index>>,
    progress: Mutex<Progress>,
    busy: AtomicBool,
}

/// Where an update reads from: the library's folders and the event cache.
pub struct Sources<'a> {
    pub lib: &'a Library<'a>,
    /// `<filesDir>/calendar`, one `<linkId>.json` per calendar link (F18-FR6).
    pub calendar: PathBuf,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `mtime-ns:size` of a file, `-` when it doesn't exist.
fn stamp_of(path: &Path) -> String {
    let Ok(m) = std::fs::metadata(path) else {
        return "-".into();
    };
    let ns = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("{ns}:{}", m.len())
}

/// A meeting's stamp: its sidecar's and its notes text's.
fn meeting_stamp(dir: &Path, id: &str) -> String {
    let sc = stamp_of(&dir.join(sidecar::sidecar_file_name(id)));
    let notes = stamp_of(&notes_text::path(dir, id));
    format!("{sc}|{notes}")
}

/// The file of one calendar link's events: `{"events": [EventDoc]}`. M13 writes it.
#[derive(serde::Deserialize)]
struct EventFile {
    #[serde(default)]
    events: Vec<serde_json::Value>,
}

/// The events in `path` that belong to `link`; anything unreadable is no events.
pub fn read_events(path: &Path, link: &str) -> Vec<EventDoc> {
    let Ok(bytes) = crate::atomic::read_capped(path, MAX_EVENT_FILE) else {
        return Vec::new();
    };
    let Ok(f) = serde_json::from_slice::<EventFile>(&bytes) else {
        return Vec::new();
    };
    let prefix = format!("{link}:");
    f.events
        .into_iter()
        .filter_map(|v| serde_json::from_value::<EventDoc>(v).ok())
        .filter(|e| e.event_key.starts_with(&prefix))
        .collect()
}

/// Every `<linkId>.json` in the event cache, by link id.
fn event_files(dir: &Path) -> HashMap<String, PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let link = name.strip_suffix(".json")?;
            let ok = !link.is_empty() && link.bytes().all(|b| b.is_ascii_alphanumeric());
            ok.then(|| (link.to_owned(), e.path()))
        })
        .collect()
}

/// An RFC 3339 time as epoch ms.
fn ms_of(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// One unit of work: a meeting, or one calendar link's events.
enum Work {
    Meeting { id: String, dir: PathBuf },
    Events { link: String, path: PathBuf },
}

impl Searcher {
    /// The index in `dir` (created on first use).
    pub fn at(dir: PathBuf) -> Searcher {
        Searcher {
            dir: Some(dir),
            ix: Mutex::new(None),
            progress: Mutex::default(),
            busy: AtomicBool::new(false),
        }
    }

    /// An index kept in memory only.
    pub fn in_memory() -> Searcher {
        Searcher {
            dir: None,
            ..Searcher::at(PathBuf::new())
        }
    }

    fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(FILE))
    }

    /// Remove the index's files (the database and SQLite's journals beside it).
    fn delete_files(&self) {
        if let Some(p) = self.path() {
            for suffix in ["", "-journal", "-wal", "-shm"] {
                let mut f = p.clone().into_os_string();
                f.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(f));
            }
        }
    }

    /// Open the index, or start it again when it can't be used (EC29). Returns whether it is
    /// new. Storage that can't hold a file at all falls back to memory, so Search still works.
    fn open(&self) -> (Index, bool) {
        let Some(p) = self.path() else {
            return (Index::in_memory(), true);
        };
        if let Some(d) = &self.dir {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(ix) = Index::open(&p) {
            let created = ix.created;
            return (ix, created);
        }
        self.delete_files();
        match Index::open(&p) {
            Ok(ix) => (ix, true),
            Err(_) => (Index::in_memory(), true),
        }
    }

    /// `f` on the open index (opened on first use).
    pub fn with<T>(&self, f: impl FnOnce(&mut Index) -> T) -> T {
        let mut g = lock(&self.ix);
        if g.is_none() {
            let (ix, created) = self.open();
            if created {
                lock(&self.progress).empty = true;
            }
            *g = Some(ix);
        }
        f(g.as_mut().expect("opened above"))
    }

    /// `nativeIndexStatus`.
    pub fn status(&self) -> Status {
        self.with(|_| ()); // open it, so a missing index reads as missing
        let p = lock(&self.progress);
        let state = if self.busy.load(Ordering::SeqCst) {
            "indexing"
        } else if p.empty {
            "missing"
        } else {
            "ready"
        };
        Status {
            state,
            done: p.done,
            total: p.total,
        }
    }

    /// The index can't be read: close it and remove its files, so the next update rebuilds it
    /// (EC29). Not while an update runs; that one finishes on what it opened.
    pub fn reset(&self) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let mut g = lock(&self.ix);
        *g = None;
        self.delete_files();
    }

    /// Whether an update or rebuild is running now ("Still indexing…").
    pub fn indexing(&self) -> bool {
        self.busy.load(Ordering::SeqCst) || lock(&self.progress).empty
    }

    /// Re-index what changed (F19-FR5). `None` when another update is already running.
    pub fn update(&self, src: &Sources<'_>) -> Option<Updated> {
        self.run(src, false)
    }

    /// Start the index again from nothing, then index everything ("Rebuild search index").
    pub fn rebuild(&self, src: &Sources<'_>) -> Option<Updated> {
        self.run(src, true)
    }

    fn run(&self, src: &Sources<'_>, rebuild: bool) -> Option<Updated> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return None;
        }
        if rebuild {
            let mut g = lock(&self.ix);
            *g = None; // closes the database before its files go
            self.delete_files();
        }
        let out = self.update_now(src, rebuild);
        self.busy.store(false, Ordering::SeqCst);
        Some(out)
    }

    fn update_now(&self, src: &Sources<'_>, rebuild: bool) -> Updated {
        let stamps = self.with(|ix| ix.stamps().unwrap_or_default());
        let mut out = Updated {
            rebuilt: rebuild,
            ..Updated::default()
        };
        let mut todo: Vec<(Work, String)> = Vec::new();
        let mut live: HashSet<String> = HashSet::new();
        for (id, f) in src.lib.scan() {
            if !f.has_sidecar {
                continue; // legacy audio has nothing to search
            }
            let key = key_of(GroupKind::Meeting, &id);
            let stamp = meeting_stamp(&f.dir, &id);
            if stamps.get(&key) != Some(&stamp) {
                todo.push((Work::Meeting { id, dir: f.dir }, stamp));
            }
            live.insert(key);
        }
        for (link, path) in event_files(&src.calendar) {
            let stamp = stamp_of(&path);
            let prefix = format!("e:{link}:");
            let mine: Vec<&String> = stamps.keys().filter(|k| k.starts_with(&prefix)).collect();
            let current = !mine.is_empty() && mine.iter().all(|k| stamps[*k] == stamp);
            if current {
                live.extend(mine.into_iter().cloned());
            } else {
                todo.push((Work::Events { link, path }, stamp));
            }
        }
        {
            let mut p = lock(&self.progress);
            (p.done, p.total) = (0, todo.len());
        }
        // Gone from the files (or changed, and about to be written again): out of the index.
        let gone: Vec<&String> = stamps.keys().filter(|k| !live.contains(*k)).collect();
        for key in gone {
            if self.with(|ix| ix.remove(key)).is_ok() {
                out.removed += 1;
            }
        }
        for (work, stamp) in todo {
            out.indexed += self.index_one(work, &stamp);
            lock(&self.progress).done += 1;
        }
        lock(&self.progress).empty = false;
        out
    }

    /// Index one meeting or one link's events; how many groups were written.
    fn index_one(&self, work: Work, stamp: &str) -> usize {
        match work {
            Work::Meeting { id, dir } => {
                let key = key_of(GroupKind::Meeting, &id);
                let Ok(sc) = sidecar::read(&dir, &id) else {
                    // Unreadable: nothing of it is searchable until it is fixed.
                    let _ = self.with(|ix| ix.remove(&key));
                    return 0;
                };
                let notes = notes_text::read(&notes_text::path(&dir, &id)).unwrap_or_default();
                // M12 hook (F19-FR7): the regions written on since recognition, from the
                // canvas strokes newer than `notesRecognisedAt`. No canvas yet: none.
                let changed: Vec<u32> = Vec::new();
                let start_ms = ms_of(&sc.started_at).unwrap_or_default();
                let g = Group {
                    kind: GroupKind::Meeting,
                    id: id.clone(),
                    title: sc.title.clone(),
                    start: sc.started_at.clone(),
                    start_ms,
                };
                let u = units::meeting_units(&sc, &notes, &changed);
                usize::from(self.with(|ix| ix.put(&g, stamp, &u)).is_ok())
            }
            // The link's old events are already out: they weren't live.
            Work::Events { link, path } => {
                let mut n = 0;
                for e in read_events(&path, &link) {
                    let g = Group {
                        kind: GroupKind::Event,
                        id: e.event_key.clone(),
                        title: Some(e.title.clone()).filter(|t| !t.trim().is_empty()),
                        start: e.start.clone(),
                        start_ms: ms_of(&e.start).unwrap_or_default(),
                    };
                    let u = units::event_units(&e);
                    n += usize::from(self.with(|ix| ix.put(&g, stamp, &u)).is_ok());
                }
                n
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_update_at_a_time_and_memory_only() {
        let s = Searcher::in_memory();
        let tz = crate::tz::FixedTz(chrono::FixedOffset::east_opt(0).unwrap());
        let lib = Library {
            folders: vec![PathBuf::from("/nonexistent/meetings")],
            tz: &tz,
        };
        let src = Sources {
            lib: &lib,
            calendar: PathBuf::from("/nonexistent/calendar"),
        };
        assert_eq!(s.status().state, "missing");
        assert!(s.indexing());
        s.busy.store(true, Ordering::SeqCst);
        assert_eq!(s.status().state, "indexing");
        assert_eq!(s.update(&src), None);
        assert_eq!(s.rebuild(&src), None);
        s.busy.store(false, Ordering::SeqCst);
        assert!(s.rebuild(&src).unwrap().rebuilt);
        assert_eq!(s.status().state, "ready");
        assert!(!s.indexing());
        assert_eq!(stamp_of(Path::new("/nonexistent/x")), "-");
        assert_eq!(ms_of("junk"), None);
    }
}
