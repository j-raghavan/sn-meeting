//! F20-FR4: the user's edits to a meeting's tags: add, remove, and rename (which the app runs
//! over every meeting in the save folders, one sidecar at a time).
use crate::sidecar::{Sidecar, Tag, TagSource};

/// Add `name` as the user's tag; it is no longer "removed by the user". Whether it was new.
pub fn add(sc: &mut Sidecar, name: &str) -> bool {
    sc.tags_user_removed.retain(|n| n != name);
    match sc.tags.iter_mut().find(|t| t.name == name) {
        Some(t) => {
            t.source = TagSource::User; // the user chose it: "Summarise again" keeps it
            false
        }
        None => {
            sc.tags.push(Tag::new(name, TagSource::User));
            true
        }
    }
}

/// Remove `name`, and remember it so a later summary doesn't bring it back (F20-AC3).
/// Whether the meeting had it.
pub fn remove(sc: &mut Sidecar, name: &str) -> bool {
    let had = sc.tags.iter().any(|t| t.name == name);
    sc.tags.retain(|t| t.name != name);
    if !sc.tags_user_removed.iter().any(|n| n == name) {
        sc.tags_user_removed.push(name.to_owned());
    }
    had
}

/// Rename `from` to `to` in one meeting, merging into `to` when it already has it (F20-AC7).
/// The result is the user's, and `from` counts as removed by the user. Whether it changed.
pub fn rename(sc: &mut Sidecar, from: &str, to: &str) -> bool {
    let Some(i) = sc.tags.iter().position(|t| t.name == from) else {
        return false;
    };
    if from == to {
        return false;
    }
    if sc.tags.iter().any(|t| t.name == to) {
        sc.tags.remove(i);
        if let Some(t) = sc.tags.iter_mut().find(|t| t.name == to) {
            t.source = TagSource::User;
        }
    } else {
        sc.tags[i].name = to.to_owned();
        sc.tags[i].source = TagSource::User;
    }
    sc.tags_user_removed.retain(|n| n != to);
    if !sc.tags_user_removed.iter().any(|n| n == from) {
        sc.tags_user_removed.push(from.to_owned());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(tags: &[(&str, TagSource)]) -> Sidecar {
        let mut sc = crate::sidecar::blank("meeting-20261007-211706");
        sc.tags = tags.iter().map(|(n, s)| Tag::new(n, *s)).collect();
        sc
    }

    fn names(sc: &Sidecar) -> Vec<(&str, TagSource)> {
        sc.tags
            .iter()
            .map(|t| (t.name.as_str(), t.source))
            .collect()
    }

    #[test]
    fn add_and_remove() {
        let mut s = sc(&[("gpu", TagSource::Ai)]);
        assert!(remove(&mut s, "gpu"));
        assert!(!remove(&mut s, "gpu"));
        assert_eq!(s.tags_user_removed, ["gpu"]);
        assert!(add(&mut s, "gpu"));
        assert!(s.tags_user_removed.is_empty());
        assert_eq!(names(&s), [("gpu", TagSource::User)]);
        let mut s = sc(&[("vram", TagSource::Ai)]);
        assert!(!add(&mut s, "vram"));
        assert_eq!(names(&s), [("vram", TagSource::User)]);
    }

    #[test]
    fn rename_merges_into_an_existing_tag() {
        let mut s = sc(&[("gpus", TagSource::Ai), ("gpu", TagSource::Ai)]);
        assert!(rename(&mut s, "gpus", "gpu"));
        assert_eq!(names(&s), [("gpu", TagSource::User)]);
        assert_eq!(s.tags_user_removed, ["gpus"]);
        let mut s = sc(&[("q4", TagSource::Ai), ("budget", TagSource::Ai)]);
        s.tags_user_removed = vec!["fourth-quarter".into()];
        assert!(rename(&mut s, "q4", "fourth-quarter"));
        assert_eq!(
            names(&s),
            [
                ("fourth-quarter", TagSource::User),
                ("budget", TagSource::Ai)
            ]
        );
        assert_eq!(s.tags_user_removed, ["q4"]);
        assert!(!rename(&mut s, "absent", "x"));
        assert!(!rename(&mut s, "budget", "budget"));
    }
}
