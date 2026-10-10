//! F17-FR10: the user's corrections to the minutes. An edit marks its field `source: "user"`
//! (`summarySource`, `decisionsSource[i]`, `sectionsSource[i]`, as `titleSource`, F5-FR5), and
//! Summarise again never replaces a user-edited field: it fills only the AI-sourced ones. A
//! meeting no one has corrected carries none of these fields (absent means AI-filled).
use crate::sidecar::{Section, Sidecar};
use serde_json::{Value, json};

/// The longest summary, decision or section body kept (a corrected field is plain text).
pub const MAX_FIELD_CHARS: usize = 20_000;
/// At most this many decisions.
pub const MAX_DECISIONS: usize = 200;

/// Which fields the user wrote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    pub summary: bool,
    pub decisions: Vec<bool>,
    pub sections: Vec<bool>,
}

fn is_user(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str) == Some("user")
}

fn list(sc: &Sidecar, key: &str, n: usize) -> Vec<bool> {
    let a = sc.extra.get(key).and_then(Value::as_array);
    (0..n).map(|i| is_user(a.and_then(|a| a.get(i)))).collect()
}

/// The meeting's sources, one per decision and section (missing entries are AI-filled).
pub fn sources(sc: &Sidecar) -> Sources {
    Sources {
        summary: is_user(sc.extra.get("summarySource")),
        decisions: list(sc, "decisionsSource", sc.decisions.len()),
        sections: list(sc, "sectionsSource", sc.sections.len()),
    }
}

/// Whether the user has corrected anything.
pub fn any_user(s: &Sources) -> bool {
    s.summary || s.decisions.iter().any(|b| *b) || s.sections.iter().any(|b| *b)
}

fn word(user: bool) -> &'static str {
    if user { "user" } else { "auto" }
}

/// Store `s` in the sidecar (Data Model: `summarySource`, `decisionsSource`, `sectionsSource`).
pub fn store(sc: &mut Sidecar, s: &Sources) {
    sc.extra
        .insert("summarySource".into(), json!(word(s.summary)));
    let words = |v: &[bool]| Value::from(v.iter().map(|b| word(*b)).collect::<Vec<_>>());
    sc.extra
        .insert("decisionsSource".into(), words(&s.decisions));
    sc.extra.insert("sectionsSource".into(), words(&s.sections));
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// F5-FR4's merge of the AI's minutes, keeping every user-edited field (F17-FR10, F17-AC7):
/// a user summary stays; user decisions stay, first, and the AI's replace the rest (one the
/// user already has is not repeated); a user section keeps its body under its heading, where
/// the AI gives that heading or, if it no longer does, after the AI's sections.
pub fn merge_ai(sc: &mut Sidecar, summary: String, decisions: Vec<String>, sections: Vec<Section>) {
    let src = sources(sc);
    if !any_user(&src) {
        sc.summary = Some(summary);
        sc.decisions = decisions;
        sc.sections = sections;
        return;
    }
    if !src.summary {
        sc.summary = Some(summary);
    }
    let kept: Vec<String> = sc
        .decisions
        .iter()
        .zip(&src.decisions)
        .filter(|(_, u)| **u)
        .map(|(d, _)| d.clone())
        .collect();
    let fresh: Vec<String> = decisions
        .into_iter()
        .filter(|d| !kept.iter().any(|k| norm(k) == norm(d)))
        .collect();
    let mut next = Sources {
        summary: src.summary,
        decisions: vec![true; kept.len()],
        sections: Vec::new(),
    };
    next.decisions.extend(vec![false; fresh.len()]);
    sc.decisions = kept.into_iter().chain(fresh).collect();
    let mut users: Vec<Section> = sc
        .sections
        .iter()
        .zip(&src.sections)
        .filter(|(_, u)| **u)
        .map(|(s, _)| s.clone())
        .collect();
    let mut out = Vec::new();
    for s in sections {
        match users
            .iter()
            .position(|u| norm(&u.heading) == norm(&s.heading))
        {
            Some(i) => {
                out.push(users.remove(i));
                next.sections.push(true);
            }
            None => {
                out.push(s);
                next.sections.push(false);
            }
        }
    }
    next.sections.extend(vec![true; users.len()]);
    out.extend(users);
    sc.sections = out;
    store(sc, &next);
}

fn text_of(v: &Value, what: &str) -> Result<String, String> {
    let t = v
        .as_str()
        .ok_or_else(|| format!("{what} must be text"))?
        .trim();
    if t.chars().count() > MAX_FIELD_CHARS {
        return Err(format!("{what} is too long"));
    }
    Ok(t.to_owned())
}

fn index_of(v: &Value, len: usize, what: &str) -> Result<usize, String> {
    let i = v
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(|| format!("{what} needs an index"))?;
    if i > len {
        return Err(format!("no {what} {i}"));
    }
    Ok(i)
}

/// `nativeEditMinutes(id, patch)`: one correction, `{"summary": text}`, `{"decision": {index,
/// text}}` (index = count appends; empty text deletes) or `{"section": {index, body}}`. The
/// field it writes becomes the user's.
pub fn edit(sc: &mut Sidecar, patch: &Value) -> Result<(), String> {
    let mut src = sources(sc);
    if let Some(v) = patch.get("summary") {
        let t = text_of(v, "the summary")?;
        sc.summary = (!t.is_empty()).then_some(t);
        src.summary = true;
    } else if let Some(d) = patch.get("decision") {
        let i = index_of(d, sc.decisions.len(), "decision")?;
        let t = text_of(d.get("text").unwrap_or(&Value::Null), "a decision")?;
        match (i == sc.decisions.len(), t.is_empty()) {
            (true, true) => return Ok(()),
            (true, false) => {
                if sc.decisions.len() >= MAX_DECISIONS {
                    return Err("too many decisions".into());
                }
                sc.decisions.push(t);
                src.decisions.push(true);
            }
            (false, true) => {
                sc.decisions.remove(i);
                src.decisions.remove(i);
            }
            (false, false) => {
                sc.decisions[i] = t;
                src.decisions[i] = true;
            }
        }
    } else if let Some(s) = patch.get("section") {
        let i = index_of(s, sc.sections.len().saturating_sub(1), "section")?;
        let Some(sec) = sc.sections.get_mut(i) else {
            return Err("no such section".into());
        };
        sec.body = text_of(s.get("body").unwrap_or(&Value::Null), "a section")?;
        src.sections[i] = true;
    } else {
        return Err("nothing to change".into());
    }
    store(sc, &src);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting() -> Sidecar {
        serde_json::from_value(json!({
            "schema": 1, "id": "meeting-20261009-100000", "startedAt": "2026-10-09T10:00:00-07:00",
            "status": "done", "summary": "AI summary.", "decisions": ["Ship Friday", "Hire two"],
            "sections": [{"heading": "Blockers", "body": "None."}, {"heading": "Risks", "body": "Budget."}]
        }))
        .unwrap()
    }

    fn sec(h: &str, b: &str) -> Section {
        Section {
            heading: h.into(),
            body: b.into(),
            extra: Default::default(),
        }
    }

    #[test]
    fn untouched_minutes_are_replaced_and_carry_no_sources() {
        let mut sc = meeting();
        merge_ai(
            &mut sc,
            "New.".into(),
            vec!["A".into()],
            vec![sec("X", "y")],
        );
        assert_eq!(sc.summary.as_deref(), Some("New."));
        assert_eq!(sc.decisions, ["A"]);
        assert!(!sc.extra.contains_key("summarySource"));
    }

    /// F17-AC7: the user's summary stays; the AI-sourced decisions are replaced.
    #[test]
    fn user_fields_survive_summarise_again() {
        let mut sc = meeting();
        edit(&mut sc, &json!({"summary": "  My own words.  "})).unwrap();
        edit(
            &mut sc,
            &json!({"decision": {"index": 1, "text": "Hire three"}}),
        )
        .unwrap();
        edit(
            &mut sc,
            &json!({"section": {"index": 1, "body": "Budget is fine."}}),
        )
        .unwrap();
        assert_eq!(sc.extra["summarySource"], "user");
        assert_eq!(sc.extra["decisionsSource"], json!(["auto", "user"]));
        let ai = vec!["Ship Monday".into(), "hire  THREE".into()];
        let secs = vec![sec("Risks", "AI risks."), sec("Owners", "Priya.")];
        merge_ai(&mut sc, "AI again.".into(), ai, secs);
        assert_eq!(sc.summary.as_deref(), Some("My own words."));
        assert_eq!(sc.decisions, ["Hire three", "Ship Monday"]);
        assert_eq!(sc.extra["decisionsSource"], json!(["user", "auto"]));
        assert_eq!(sc.sections[0], sec("Risks", "Budget is fine."));
        assert_eq!(sc.sections[1], sec("Owners", "Priya."));
        assert_eq!(sc.extra["sectionsSource"], json!(["user", "auto"]));
        // A user section the AI no longer gives goes after the AI's.
        merge_ai(&mut sc, "x".into(), vec![], vec![sec("Owners", "Sam.")]);
        assert_eq!(
            sc.sections,
            [sec("Owners", "Sam."), sec("Risks", "Budget is fine.")]
        );
        assert_eq!(sc.extra["sectionsSource"], json!(["auto", "user"]));
        // An AI-only meeting the user edited a decision of: the summary is the AI's again.
        let mut m = meeting();
        edit(
            &mut m,
            &json!({"decision": {"index": 0, "text": "Ship Friday!"}}),
        )
        .unwrap();
        merge_ai(&mut m, "Fresh.".into(), vec![], vec![]);
        assert_eq!(m.summary.as_deref(), Some("Fresh."));
        assert_eq!(m.extra["summarySource"], "auto");
    }

    #[test]
    fn decisions_are_added_changed_and_deleted() {
        let mut sc = meeting();
        edit(&mut sc, &json!({"decision": {"index": 2, "text": "Third"}})).unwrap();
        assert_eq!(sc.decisions.len(), 3);
        edit(&mut sc, &json!({"decision": {"index": 3, "text": ""}})).unwrap();
        assert_eq!(sc.decisions.len(), 3, "an empty append is nothing");
        edit(&mut sc, &json!({"decision": {"index": 0, "text": ""}})).unwrap();
        assert_eq!(sc.decisions, ["Hire two", "Third"]);
        assert_eq!(sc.extra["decisionsSource"], json!(["auto", "user"]));
        edit(&mut sc, &json!({"summary": ""})).unwrap();
        assert_eq!(sc.summary, None);
        sc.decisions = vec!["d".into(); MAX_DECISIONS];
        let full = edit(
            &mut sc,
            &json!({"decision": {"index": MAX_DECISIONS, "text": "x"}}),
        );
        assert!(full.is_err());
    }

    #[test]
    fn bad_edits_are_refused() {
        let mut sc = meeting();
        for bad in [
            json!({}),
            json!({"summary": 3}),
            json!({"summary": "x".repeat(MAX_FIELD_CHARS + 1)}),
            json!({"decision": {"text": "no index"}}),
            json!({"decision": {"index": 9, "text": "x"}}),
            json!({"decision": {"index": 0}}),
            json!({"section": {"index": 2, "body": "x"}}),
            json!({"section": {"index": 0, "body": 1}}),
        ] {
            assert!(edit(&mut sc, &bad).is_err(), "{bad}");
        }
        sc.sections.clear();
        assert!(edit(&mut sc, &json!({"section": {"index": 0, "body": "x"}})).is_err());
        assert_eq!(
            sources(&meeting()),
            Sources {
                summary: false,
                decisions: vec![false, false],
                sections: vec![false, false],
            }
        );
    }
}
