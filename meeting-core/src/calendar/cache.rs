//! F18-FR6: the expanded events of one link, in `<filesDir>/calendar/<linkId>.json`, app-private
//! (Partner never syncs it). It holds the events, the validators, when the link was last
//! fetched and how that went, and the calendar's name: never the URL (INV15). Home renders
//! from it, so the agenda works offline (F18-AC9). Search reads the same file (M14).
use super::ics::Event;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A cache file is at most a few MB (2 000 instances per series); bigger is not ours.
const MAX_CACHE_BYTES: u64 = 8 * 1024 * 1024;

/// How the last fetch of a link went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkStatus {
    /// Never fetched.
    #[default]
    Never,
    Ok,
    /// 401, 403 or 404: "This calendar link no longer works" (EC23).
    Gone,
    /// Anything else (no connection, timed out, not a calendar…).
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CacheFile {
    pub link_id: String,
    /// `X-WR-CALNAME`, when the feed has one.
    pub calendar: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// The last successful fetch (200 or 304), epoch ms; 0 = never.
    pub fetched_at_ms: i64,
    /// The last attempt, successful or not; 0 = never.
    pub attempted_at_ms: i64,
    pub status: LinkStatus,
    /// Why the last attempt failed, URL-free and scrubbed.
    pub reason: Option<String>,
    /// Events the feed had that couldn't be read (F18-FR5).
    pub skipped: usize,
    pub events: Vec<Event>,
}

/// `<dir>/<linkId>.json`.
pub fn path(dir: &Path, link_id: &str) -> PathBuf {
    dir.join(format!("{link_id}.json"))
}

/// The cache of `link_id`; anything missing or unreadable is an empty, never-fetched one.
pub fn read(dir: &Path, link_id: &str) -> CacheFile {
    crate::atomic::read_capped(&path(dir, link_id), MAX_CACHE_BYTES)
        .ok()
        .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok())
        .filter(|c| c.link_id == link_id)
        .unwrap_or_else(|| CacheFile {
            link_id: link_id.to_owned(),
            ..CacheFile::default()
        })
}

/// Write atomically, creating the folder.
pub fn write(dir: &Path, c: &CacheFile) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec(c).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(&path(dir, &c.link_id), &bytes)
}

/// A link id: 8 lowercase hex characters (F18-FR2).
pub fn is_link_id(s: &str) -> bool {
    s.len() == 8
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Remove the caches of links no longer in the file, so their events leave Home and Search.
/// Only `<8 hex>.json` files are touched. Returns how many were removed.
pub fn prune(dir: &Path, keep: &[&str]) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if is_link_id(id) && !keep.contains(&id) && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_damage() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("calendar");
        assert_eq!(read(&dir, "0123abcd").status, LinkStatus::Never);
        let c = CacheFile {
            link_id: "0123abcd".into(),
            calendar: Some("Work".into()),
            etag: Some("\"v1\"".into()),
            fetched_at_ms: 5,
            attempted_at_ms: 5,
            status: LinkStatus::Ok,
            ..CacheFile::default()
        };
        write(&dir, &c).unwrap();
        assert_eq!(read(&dir, "0123abcd"), c);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path(&dir, "0123abcd")).unwrap()).unwrap();
        assert_eq!(v["events"], serde_json::json!([]), "the shape Search reads");
        assert_eq!(v["status"], "ok");
        // Another link's file under this name, or junk, is no cache.
        std::fs::write(path(&dir, "ffffffff"), b"{\"linkId\":\"0123abcd\"}").unwrap();
        assert_eq!(read(&dir, "ffffffff").link_id, "ffffffff");
        assert_eq!(read(&dir, "ffffffff").status, LinkStatus::Never);
        std::fs::write(path(&dir, "eeeeeeee"), b"junk").unwrap();
        std::fs::write(dir.join("notes.json"), b"{}").unwrap();
        assert_eq!(prune(&dir, &["0123abcd"]), 2);
        assert!(path(&dir, "0123abcd").exists());
        assert!(dir.join("notes.json").exists(), "not a link's file");
        assert_eq!(prune(&d.path().join("missing"), &[]), 0);
    }

    #[test]
    fn link_ids() {
        assert!(is_link_id("0123abcd"));
        assert!(!is_link_id("0123ABCD"));
        assert!(!is_link_id("0123abc"));
        assert!(!is_link_id("0123abcg"));
    }
}
