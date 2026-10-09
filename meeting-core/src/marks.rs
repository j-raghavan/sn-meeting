//! F15-FR11: Marks, timestamped markers the user drops with the Mark button while recording.
//! They live in the sidecar's optional `marks` list (schema 1), `{id, atMs, label}`, with
//! `atMs` on the recording clock (INV13). The list is read leniently, as Search reads it: an
//! entry that isn't a mark is kept as written and skipped (F3-FR6, F16-FR8's spirit).
use crate::pipeline::marker;
use crate::sidecar::Sidecar;
use serde::Serialize;
use serde_json::{Value, json};

/// The most marks one meeting keeps.
pub const MAX_MARKS: usize = 1_000;
/// The longest label.
pub const MAX_LABEL: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub id: String,
    pub at_ms: u64,
    pub label: Option<String>,
}

fn list_mut(sc: &mut Sidecar) -> &mut Vec<Value> {
    let v = sc
        .extra
        .entry("marks")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !v.is_array() {
        *v = Value::Array(Vec::new()); // not a list at all: nothing in it was a mark
    }
    v.as_array_mut().expect("made an array above")
}

/// The meeting's marks, by time (ties in the order they were added).
pub fn list(sc: &Sidecar) -> Vec<Mark> {
    let Some(all) = sc.extra.get("marks").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<Mark> = all
        .iter()
        .filter_map(|m| {
            Some(Mark {
                id: m.get("id")?.as_str()?.to_owned(),
                at_ms: m.get("atMs")?.as_u64()?,
                label: m.get("label").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    out.sort_by_key(|m| m.at_ms);
    out
}

/// A fresh mark id, `m-` and 8 hex digits.
fn new_id() -> String {
    format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

/// `nativeAddMark`: a mark at `at_ms`, unlabelled. Its id, or why not.
pub fn add(sc: &mut Sidecar, at_ms: i64) -> Result<String, String> {
    let at = u64::try_from(at_ms).map_err(|_| "atMs must be >= 0".to_owned())?;
    if list(sc).len() >= MAX_MARKS {
        return Err(format!("at most {MAX_MARKS} marks"));
    }
    let id = new_id();
    list_mut(sc).push(json!({"id": id, "atMs": at, "label": null}));
    Ok(id)
}

/// `nativeSetMarkLabel`: one line of at most [`MAX_LABEL`] characters; "" clears it.
pub fn set_label(sc: &mut Sidecar, mark_id: &str, label: &str) -> Result<(), String> {
    let one: String = label
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one = one.trim();
    if one.chars().count() > MAX_LABEL {
        return Err(format!("a label is at most {MAX_LABEL} characters"));
    }
    let m = list_mut(sc)
        .iter_mut()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(mark_id))
        .ok_or_else(|| "mark not found".to_owned())?;
    m["label"] = if one.is_empty() {
        Value::Null
    } else {
        json!(one)
    };
    Ok(())
}

/// `nativeDeleteMark`.
pub fn delete(sc: &mut Sidecar, mark_id: &str) -> Result<(), String> {
    let all = list_mut(sc);
    let before = all.len();
    all.retain(|m| m.get("id").and_then(Value::as_str) != Some(mark_id));
    if all.len() == before {
        return Err("mark not found".into());
    }
    Ok(())
}

/// F15-FR11: a mark as its own transcript line, `◆ [0:12:41] Mark` (or its label).
pub fn line(m: &Mark) -> String {
    format!(
        "◆ [{}] {}",
        marker(m.at_ms),
        m.label.as_deref().unwrap_or("Mark")
    )
}

/// F15-FR11 / F17-FR9: the marks as the summarise request lists them, after the transcript.
pub fn summary_block(sc: &Sidecar) -> Option<String> {
    let marks = list(sc);
    if marks.is_empty() {
        return None;
    }
    let rows: Vec<String> = marks
        .iter()
        .map(|m| {
            format!(
                "[{}] {}",
                marker(m.at_ms),
                m.label.as_deref().unwrap_or("(no label)")
            )
        })
        .collect();
    Some(format!(
        "The user marked these moments:\n<marks>\n{}\n</marks>",
        rows.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc() -> Sidecar {
        crate::sidecar::blank("meeting-20261009-100000")
    }

    #[test]
    fn add_label_and_delete() {
        let mut s = sc();
        let a = add(&mut s, 761_000).unwrap();
        let b = add(&mut s, 5_000).unwrap();
        assert!(a.starts_with("m-") && a.len() == 10 && a != b);
        assert_eq!(
            list(&s).iter().map(|m| m.at_ms).collect::<Vec<_>>(),
            [5_000, 761_000]
        );
        set_label(&mut s, &a, "  Budget\ndecision ").unwrap();
        assert_eq!(list(&s)[1].label.as_deref(), Some("Budget decision"));
        assert_eq!(line(&list(&s)[1]), "◆ [0:12:41] Budget decision");
        assert_eq!(line(&list(&s)[0]), "◆ [0:00:05] Mark");
        set_label(&mut s, &a, "").unwrap();
        assert_eq!(list(&s)[1].label, None);
        assert_eq!(
            set_label(&mut s, "m-nope", "x"),
            Err("mark not found".into())
        );
        assert!(set_label(&mut s, &a, &"x".repeat(201)).is_err());
        delete(&mut s, &b).unwrap();
        assert_eq!(delete(&mut s, &b), Err("mark not found".into()));
        assert_eq!(list(&s).len(), 1);
        assert_eq!(add(&mut s, -1), Err("atMs must be >= 0".into()));
    }

    #[test]
    fn entries_that_are_not_marks_are_kept_and_skipped() {
        let mut s = sc();
        s.extra
            .insert("marks".into(), json!(["junk", {"id": "m-1", "atMs": 3}]));
        assert_eq!(
            list(&s),
            [Mark {
                id: "m-1".into(),
                at_ms: 3,
                label: None
            }]
        );
        add(&mut s, 4).unwrap();
        assert_eq!(s.extra["marks"][0], "junk");
        s.extra.insert("marks".into(), json!("not a list"));
        assert!(list(&s).is_empty());
        add(&mut s, 1).unwrap();
        assert_eq!(list(&s).len(), 1);
    }

    #[test]
    fn at_most_a_thousand() {
        let mut s = sc();
        for i in 0..MAX_MARKS {
            add(&mut s, i as i64).unwrap();
        }
        assert!(add(&mut s, 1).is_err());
    }

    #[test]
    fn the_summary_lists_them() {
        let mut s = sc();
        assert_eq!(summary_block(&s), None);
        let a = add(&mut s, 761_000).unwrap();
        add(&mut s, 30_000).unwrap();
        set_label(&mut s, &a, "Budget").unwrap();
        assert_eq!(
            summary_block(&s).unwrap(),
            "The user marked these moments:\n<marks>\n[0:00:30] (no label)\n[0:12:41] Budget\n</marks>"
        );
    }
}
