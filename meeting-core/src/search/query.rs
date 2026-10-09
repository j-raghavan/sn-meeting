//! F19-FR4's matching: the user's words become an FTS5 query (case- and accent-insensitive
//! through the tokenizer, a prefix on the last term, phrases in quotes), run on the local index
//! only (F19-FR10).
use super::index::{Group, GroupKind, Index};
use super::units::Kind;
use serde::Serialize;
use std::collections::HashMap;

/// The snippet's emphasis markers (control characters never reach the index, `units::clean`).
pub const BOLD_ON: char = '\u{2}';
pub const BOLD_OFF: char = '\u{3}';

/// Longest query read; the rest is ignored.
pub const MAX_QUERY_CHARS: usize = 200;

/// One quoted FTS5 string: `"` doubled inside.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The FTS5 query for what the user typed, or `None` when it has no word to look for.
/// `budg` → `"budg"*`; `q3 "budget review" plan` → `"q3" "budget review" "plan"*`. Every term
/// is quoted, so FTS5 operators and punctuation typed by the user are plain text.
pub fn fts_query(user: &str) -> Option<String> {
    let user: String = user.chars().take(MAX_QUERY_CHARS).collect();
    let mut terms: Vec<(String, bool)> = Vec::new(); // (text, is a phrase)
    for (i, part) in user.split('"').enumerate() {
        if i % 2 == 1 {
            if has_word(part) {
                terms.push((part.trim().to_owned(), true));
            }
        } else {
            terms.extend(
                part.split_whitespace()
                    .filter(|w| has_word(w))
                    .map(|w| (w.to_owned(), false)),
            );
        }
    }
    let n = terms.len();
    let q: Vec<String> = terms
        .into_iter()
        .enumerate()
        .map(|(i, (t, phrase))| {
            let star = if i + 1 == n && !phrase { "*" } else { "" };
            format!("{}{star}", quoted(&t))
        })
        .collect();
    (!q.is_empty()).then(|| q.join(" "))
}

/// A term the tokenizer keeps something of (a letter or a digit).
fn has_word(s: &str) -> bool {
    s.chars().any(char::is_alphanumeric)
}

/// A matching unit, best first within its group.
#[derive(Clone, Debug, PartialEq)]
pub struct Matched {
    pub row: i64,
    pub group: String,
    pub kind: Kind,
    /// bm25: lower is better.
    pub rank: f64,
}

/// Every unit matching `fts`, of `kind` when given.
pub fn matching(ix: &Index, fts: &str, kind: Option<Kind>) -> rusqlite::Result<Vec<Matched>> {
    let mut st = ix.conn.prepare(
        "SELECT u.rowid, u.grp, u.kind, bm25(body) FROM body JOIN units u ON u.rowid = body.rowid
         WHERE body MATCH ?1 AND (?2 IS NULL OR u.kind = ?2) ORDER BY bm25(body)",
    )?;
    let rows = st.query_map(rusqlite::params![fts, kind.map(Kind::id)], |r| {
        let kind: String = r.get(2)?;
        Ok(Matched {
            row: r.get(0)?,
            group: r.get(1)?,
            kind: Kind::from_id(&kind).unwrap_or(Kind::Minutes),
            rank: r.get(3)?,
        })
    })?;
    rows.collect()
}

/// The filter chips (F19-FR1): All, or one source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    All,
    Notes,
    Transcript,
    Minutes,
    Todos,
    Events,
}

impl Filter {
    pub fn from_id(s: &str) -> Option<Filter> {
        Some(match s {
            "all" => Filter::All,
            "notes" => Filter::Notes,
            "transcript" => Filter::Transcript,
            "minutes" => Filter::Minutes,
            "todos" => Filter::Todos,
            "events" => Filter::Events,
            _ => return None,
        })
    }

    fn kind(self) -> Option<Kind> {
        match self {
            Filter::All => None,
            Filter::Notes => Some(Kind::Notes),
            Filter::Transcript => Some(Kind::Transcript),
            Filter::Minutes => Some(Kind::Minutes),
            Filter::Todos => Some(Kind::Todo),
            Filter::Events => Some(Kind::Event),
        }
    }
}

/// At most this many groups per reply (F19-FR2); "Show more" asks again with a larger limit.
pub const DEFAULT_GROUPS: usize = 50;
/// The best hits shown per group; the rest are counted.
pub const MAX_HITS_PER_GROUP: usize = 20;

/// The moment results are ordered against: now, and the start of today, both epoch ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub now_ms: i64,
    pub today_start_ms: i64,
}

/// One run of a snippet, emphasised or not (bold, no colour, F9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Span {
    pub text: String,
    pub bold: bool,
}

/// One hit (F19-FR2): its source label, snippet, and where it opens (F19-FR3).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub kind: Kind,
    pub label: &'static str,
    pub unit_id: String,
    pub at_ms: Option<u64>,
    /// `0:12:41` for a timed hit.
    pub time: Option<String>,
    pub snippet: Vec<Span>,
    pub region: Option<u32>,
    pub bbox: Option<[f64; 4]>,
    pub stroke_ids: Vec<u32>,
    /// "(notes changed since recognised)" (F19-FR7).
    pub changed: bool,
}

/// A meeting's or an event's hits.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupHits {
    pub kind: GroupKind,
    /// The meeting id or the event key.
    pub id: String,
    pub title: Option<String>,
    pub start: String,
    pub start_ms: i64,
    /// Every matching unit in the group; `hits` holds the best of them.
    pub hit_count: usize,
    pub hits: Vec<Hit>,
}

/// `nativeSearch`'s reply.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub query: String,
    pub filter: Filter,
    pub groups: Vec<GroupHits>,
    /// More groups than `limit` matched.
    pub more: bool,
    pub total_groups: usize,
    /// An update or rebuild is running: "Still indexing…" (F19-FR5).
    pub indexing: bool,
}

/// F19-FR2's order: today's groups newest first, then upcoming events by start, then earlier
/// days newest first.
fn order_key(g: &Group, c: Clock) -> (u8, i64) {
    if g.start_ms > c.now_ms {
        (1, g.start_ms)
    } else if g.start_ms >= c.today_start_ms {
        (0, -g.start_ms)
    } else {
        (2, -g.start_ms)
    }
}

/// Split a snippet with [`BOLD_ON`]/[`BOLD_OFF`] markers into runs.
pub fn spans(snippet: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut bold = false;
    let mut cur = String::new();
    let mut flush = |cur: &mut String, bold: bool| {
        if !cur.is_empty() {
            out.push(Span {
                text: std::mem::take(cur),
                bold,
            });
        }
    };
    for c in snippet.chars() {
        match c {
            BOLD_ON | BOLD_OFF => {
                flush(&mut cur, bold);
                bold = c == BOLD_ON;
            }
            '\n' => cur.push(' '),
            c => cur.push(c),
        }
    }
    flush(&mut cur, bold);
    out
}

/// F19-FR2: the matches for `user` under `filter`, grouped by meeting or event, at most
/// `limit` groups. A query with no word gives no groups.
pub fn search(
    ix: &Index,
    user: &str,
    filter: Filter,
    limit: usize,
    clock: Clock,
) -> rusqlite::Result<Results> {
    let mut r = Results {
        query: user.to_owned(),
        filter,
        groups: Vec::new(),
        more: false,
        total_groups: 0,
        indexing: false,
    };
    let Some(fts) = fts_query(user) else {
        return Ok(r);
    };
    // Rows best first, so each group's list keeps that order.
    let mut by_group: HashMap<String, Vec<i64>> = HashMap::new();
    for m in matching(ix, &fts, filter.kind())? {
        by_group.entry(m.group).or_default().push(m.row);
    }
    let mut groups: Vec<(Group, Vec<i64>)> = Vec::new();
    for (key, rows) in by_group {
        if let Some(g) = ix.group(&key)? {
            groups.push((g, rows));
        }
    }
    groups
        .sort_by(|a, b| (order_key(&a.0, clock), &a.0.id).cmp(&(order_key(&b.0, clock), &b.0.id)));
    r.total_groups = groups.len();
    r.more = groups.len() > limit;
    groups.truncate(limit);
    let shown: Vec<i64> = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().take(MAX_HITS_PER_GROUP).copied())
        .collect();
    let mut hits = details(ix, &fts, &shown)?;
    for (g, rows) in groups {
        r.groups.push(GroupHits {
            kind: g.kind,
            id: g.id,
            title: g.title,
            start: g.start,
            start_ms: g.start_ms,
            hit_count: rows.len(),
            hits: rows
                .iter()
                .take(MAX_HITS_PER_GROUP)
                .filter_map(|row| hits.remove(row))
                .collect(),
        });
    }
    Ok(r)
}

/// Each row's hit, with its snippet for `fts`.
fn details(ix: &Index, fts: &str, rows: &[i64]) -> rusqlite::Result<HashMap<i64, Hit>> {
    let mut st = ix.conn.prepare(
        "SELECT u.rowid, u.kind, u.unit_id, u.at_ms, u.region, u.bbox, u.strokes, u.changed,
                snippet(body, 0, char(2), char(3), '…', 24)
         FROM body JOIN units u ON u.rowid = body.rowid
         WHERE body MATCH ?1 AND u.rowid IN (SELECT value FROM json_each(?2))",
    )?;
    let list = serde_json::json!(rows).to_string();
    let found = st.query_map(rusqlite::params![fts, list], |r| {
        let kind = Kind::from_id(&r.get::<_, String>(1)?).unwrap_or(Kind::Minutes);
        let at: Option<i64> = r.get(3)?;
        let at_ms = at.and_then(|t| u64::try_from(t).ok());
        let bbox: Option<String> = r.get(5)?;
        let strokes: String = r.get(6)?;
        let hit = Hit {
            kind,
            label: kind.label(),
            unit_id: r.get(2)?,
            at_ms,
            time: at_ms.map(crate::pipeline::marker),
            snippet: spans(&r.get::<_, String>(8)?),
            region: r.get(4)?,
            bbox: bbox.and_then(|b| serde_json::from_str(&b).ok()),
            stroke_ids: serde_json::from_str(&strokes).unwrap_or_default(),
            changed: r.get(7)?,
        };
        Ok((r.get::<_, i64>(0)?, hit))
    })?;
    found.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_quote_every_term_and_prefix_the_last() {
        assert_eq!(fts_query("budg").as_deref(), Some("\"budg\"*"));
        assert_eq!(
            fts_query(r#"q3 "budget review" plan"#).as_deref(),
            Some(r#""q3" "budget review" "plan"*"#)
        );
        assert_eq!(
            fts_query(r#"plan "budget review""#).as_deref(),
            Some(r#""plan" "budget review""#)
        );
        assert_eq!(
            fts_query("NEAR( a OR b* )").as_deref(),
            Some(r#""NEAR(" "a" "OR" "b*"*"#)
        );
        assert_eq!(fts_query("  \"\" -- ! "), None);
        assert_eq!(fts_query(""), None);
        let long = "a".repeat(500);
        assert_eq!(fts_query(&long).unwrap().len(), MAX_QUERY_CHARS + 3);
        assert_eq!(fts_query(r#"say "it""#).as_deref(), Some(r#""say" "it""#));
        assert_eq!(quoted("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn snippets_become_runs() {
        let s = spans("a \u{2}Budget\u{3} line\nnext \u{2}b\u{3}");
        let want = [
            ("a ", false),
            ("Budget", true),
            (" line next ", false),
            ("b", true),
        ];
        let got: Vec<(&str, bool)> = s.iter().map(|s| (s.text.as_str(), s.bold)).collect();
        assert_eq!(got, want);
        assert!(spans("").is_empty());
    }

    #[test]
    fn filters_read_their_ids() {
        for (id, f) in [
            ("all", Filter::All),
            ("notes", Filter::Notes),
            ("transcript", Filter::Transcript),
            ("minutes", Filter::Minutes),
            ("todos", Filter::Todos),
            ("events", Filter::Events),
        ] {
            assert_eq!(Filter::from_id(id), Some(f));
            assert_eq!(serde_json::json!(f), id);
        }
        assert_eq!(Filter::from_id("todo"), None);
    }
}
