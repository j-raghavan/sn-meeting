//! F19-FR4's matching: the user's words become an FTS5 query (case- and accent-insensitive
//! through the tokenizer, a prefix on the last term, phrases in quotes), run on the local index
//! only (F19-FR10).
use super::index::Index;
use super::units::Kind;

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
}
