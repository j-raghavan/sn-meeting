//! F19-FR4 / ADR D19: the index. One FTS5 `unicode61` table (`remove_diacritics 2`, prefix
//! indexes of 2 and 3 characters) holds the text, beside ordinary tables of groups (a meeting or
//! an event, with what its files looked like when indexed) and unit metadata. A cache: every
//! row is rebuilt from the files (INV17).
use super::units::{Kind, Unit};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;

/// Bumped whenever the tables or what goes in them change: an older index is rebuilt (F19-FR5).
pub const VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE groups(
    key TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    title TEXT,
    start TEXT NOT NULL,
    start_ms INTEGER NOT NULL,
    stamp TEXT NOT NULL
);
CREATE TABLE units(
    rowid INTEGER PRIMARY KEY,
    grp TEXT NOT NULL,
    kind TEXT NOT NULL,
    unit_id TEXT NOT NULL,
    at_ms INTEGER,
    region INTEGER,
    bbox TEXT,
    strokes TEXT NOT NULL,
    changed INTEGER NOT NULL
);
CREATE INDEX units_grp ON units(grp);
CREATE VIRTUAL TABLE body USING fts5(text, tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3');
";

/// Whether a group is a meeting or an event (F19-FR2's grouping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Meeting,
    Event,
}

impl GroupKind {
    fn id(self) -> &'static str {
        match self {
            GroupKind::Meeting => "meeting",
            GroupKind::Event => "event",
        }
    }
}

/// A meeting or an event, as a result group shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub kind: GroupKind,
    /// The meeting id, or the event key.
    pub id: String,
    pub title: Option<String>,
    /// RFC 3339, as stored in the sidecar or the event.
    pub start: String,
    pub start_ms: i64,
}

impl Group {
    /// The group's key in the index: `m:<meeting id>` or `e:<event key>`.
    pub fn key(&self) -> String {
        key_of(self.kind, &self.id)
    }
}

pub fn key_of(kind: GroupKind, id: &str) -> String {
    match kind {
        GroupKind::Meeting => format!("m:{id}"),
        GroupKind::Event => format!("e:{id}"),
    }
}

/// The open index.
pub struct Index {
    pub(crate) conn: Connection,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Index")
    }
}

impl Index {
    /// Open the index at `path`, creating it when absent. An index of another version, or one
    /// that isn't a readable index, is an error: the caller deletes it and starts again.
    pub fn open(path: &Path) -> rusqlite::Result<Index> {
        Index::setup(Connection::open(path)?)
    }

    /// An index in memory (tests, and a fallback when storage fails).
    pub fn in_memory() -> Index {
        Index::setup(Connection::open_in_memory().expect("sqlite in memory"))
            .expect("schema in memory")
    }

    fn setup(conn: Connection) -> rusqlite::Result<Index> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
        if v == 0 && tables == 0 {
            conn.execute_batch(&format!(
                "BEGIN; {SCHEMA} PRAGMA user_version = {VERSION}; COMMIT;"
            ))?;
        } else if v != VERSION {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(Index { conn })
    }

    /// Replace everything indexed for `g` with `units`, remembering `stamp` (what its files
    /// looked like). One transaction: a search sees the old units or the new ones.
    pub fn put(&mut self, g: &Group, stamp: &str, units: &[Unit]) -> rusqlite::Result<()> {
        let key = g.key();
        let tx = self.conn.transaction()?;
        remove_in(&tx, &key)?;
        tx.execute(
            "INSERT INTO groups(key, kind, id, title, start, start_ms, stamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![key, g.kind.id(), g.id, g.title, g.start, g.start_ms, stamp],
        )?;
        {
            let mut unit = tx.prepare(
                "INSERT INTO units(grp, kind, unit_id, at_ms, region, bbox, strokes, changed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut text = tx.prepare("INSERT INTO body(rowid, text) VALUES (?1, ?2)")?;
            for u in units {
                let bbox = u.bbox.map(|b| serde_json::json!(b).to_string());
                let strokes = serde_json::json!(u.stroke_ids).to_string();
                let at = u.at_ms.map(|t| t as i64);
                let row = unit.insert(params![
                    key,
                    u.kind.id(),
                    u.unit_id,
                    at,
                    u.region,
                    bbox,
                    strokes,
                    u.changed
                ])?;
                text.execute(params![row, u.text])?;
            }
        }
        tx.commit()
    }

    /// Drop `key`'s group and units.
    pub fn remove(&mut self, key: &str) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        remove_in(&tx, key)?;
        tx.commit()
    }

    /// Every indexed group's key and stamp.
    pub fn stamps(&self) -> rusqlite::Result<HashMap<String, String>> {
        let mut st = self.conn.prepare("SELECT key, stamp FROM groups")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// The group stored under `key`.
    pub fn group(&self, key: &str) -> rusqlite::Result<Option<Group>> {
        self.conn
            .query_row(
                "SELECT kind, id, title, start, start_ms FROM groups WHERE key = ?1",
                [key],
                |r| {
                    let kind: String = r.get(0)?;
                    Ok(Group {
                        kind: if kind == "event" {
                            GroupKind::Event
                        } else {
                            GroupKind::Meeting
                        },
                        id: r.get(1)?,
                        title: r.get(2)?,
                        start: r.get(3)?,
                        start_ms: r.get(4)?,
                    })
                },
            )
            .optional()
    }

    /// How many units of `kind` are indexed (tests and status).
    pub fn count(&self, kind: Option<Kind>) -> rusqlite::Result<i64> {
        match kind {
            None => self
                .conn
                .query_row("SELECT count(*) FROM units", [], |r| r.get(0)),
            Some(k) => self.conn.query_row(
                "SELECT count(*) FROM units WHERE kind = ?1",
                [k.id()],
                |r| r.get(0),
            ),
        }
    }
}

fn remove_in(tx: &rusqlite::Transaction<'_>, key: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM body WHERE rowid IN (SELECT rowid FROM units WHERE grp = ?1)",
        [key],
    )?;
    tx.execute("DELETE FROM units WHERE grp = ?1", [key])?;
    tx.execute("DELETE FROM groups WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(id: &str) -> Group {
        Group {
            kind: GroupKind::Meeting,
            id: id.into(),
            title: Some("T".into()),
            start: "2026-10-07T21:17:06-07:00".into(),
            start_ms: 1,
        }
    }

    fn unit(text: &str) -> Unit {
        Unit {
            kind: Kind::Minutes,
            unit_id: "summary".into(),
            at_ms: None,
            region: None,
            bbox: Some([0.0, 1.0, 2.0, 3.0]),
            stroke_ids: vec![1],
            changed: false,
            text: text.into(),
        }
    }

    #[test]
    fn put_replaces_and_remove_drops() {
        let mut ix = Index::in_memory();
        ix.put(&g("a"), "s1", &[unit("one"), unit("two")]).unwrap();
        ix.put(&g("b"), "s2", &[unit("three")]).unwrap();
        assert_eq!(ix.count(None).unwrap(), 3);
        ix.put(&g("a"), "s3", &[unit("four")]).unwrap();
        assert_eq!(ix.count(None).unwrap(), 2);
        assert_eq!(ix.count(Some(Kind::Minutes)).unwrap(), 2);
        assert_eq!(ix.count(Some(Kind::Notes)).unwrap(), 0);
        assert_eq!(ix.stamps().unwrap()["m:a"], "s3");
        assert_eq!(ix.group("m:a").unwrap(), Some(g("a")));
        ix.remove("m:a").unwrap();
        assert_eq!(ix.group("m:a").unwrap(), None);
        assert_eq!(ix.count(None).unwrap(), 1);
        let n: i64 = ix
            .conn
            .query_row("SELECT count(*) FROM body", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "the text rows go with their units");
    }

    #[test]
    fn another_version_is_refused_and_a_file_reopens() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.sqlite");
        let mut ix = Index::open(&p).unwrap();
        ix.put(&g("a"), "s", &[unit("x")]).unwrap();
        drop(ix);
        assert_eq!(Index::open(&p).unwrap().count(None).unwrap(), 1);
        let c = Connection::open(&p).unwrap();
        c.execute_batch("PRAGMA user_version = 99").unwrap();
        drop(c);
        assert!(Index::open(&p).is_err());
        std::fs::write(&p, b"not a database at all, just some bytes.....").unwrap();
        assert!(Index::open(&p).is_err());
        assert_eq!(key_of(GroupKind::Event, "k"), "e:k");
    }
}
