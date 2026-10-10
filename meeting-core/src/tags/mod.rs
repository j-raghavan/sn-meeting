//! F20: tags. Each meeting gets keyword tags from its Stop call (ADR D20); they are stored in
//! the sidecar, searchable, editable and drawn as the Tag Graph.

pub mod edit;
pub mod graph;

use crate::sidecar::{Sidecar, Tag, TagSource};
use std::collections::HashMap;

/// The longest tag, in characters (F20-FR2).
pub const MAX_TAG_CHARS: usize = 32;
/// F20-FR1: the summarise call asks for 3–7 tags; more than 7 are cut.
pub const MAX_AI_TAGS: usize = 7;
/// F20-FR1 / OQ29: the most used tags sent with the call, by name only.
pub const VOCABULARY: usize = 200;

/// F20-FR2: a tag's stored form, or `None` when nothing of it is left. Lowercase, without a
/// leading `#`, words joined by `-`; ASCII letters, digits and `-`, plus the letters of other
/// scripts; 1–32 characters. AI and user input go through the same rule. Apostrophes are
/// dropped ("Ana's" → `anas`); any other character is a word break ("Local AI" and
/// "local_ai" → `local-ai`, "v1.2" → `v1-2`).
pub fn normalise(input: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization;
    let lower: String = input.nfc().collect::<String>().to_lowercase();
    let mut out = String::new();
    let mut gap = false;
    for c in lower.trim().trim_start_matches('#').chars() {
        let keep =
            c.is_ascii_lowercase() || c.is_ascii_digit() || (!c.is_ascii() && c.is_alphabetic());
        if keep {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else if !matches!(c, '\'' | '’') {
            gap = true;
        }
    }
    let cut: String = out.chars().take(MAX_TAG_CHARS).collect();
    let cut = cut.trim_end_matches('-');
    (!cut.is_empty()).then(|| cut.to_owned())
}

/// The tag as it is shown: `#local-ai`.
pub fn shown(name: &str) -> String {
    format!("#{name}")
}

/// Normalise every name and keep the first of each (F20-FR2: no duplicates within a meeting).
pub fn normalise_all<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names.into_iter().filter_map(normalise) {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// How many meetings use each tag.
pub fn counts<'a>(sidecars: impl IntoIterator<Item = &'a Sidecar>) -> HashMap<String, usize> {
    let mut out: HashMap<String, usize> = HashMap::new();
    for sc in sidecars {
        for t in &sc.tags {
            *out.entry(t.name.clone()).or_default() += 1;
        }
    }
    out
}

/// Tag names, most used first, then by name.
pub fn by_use(counts: &HashMap<String, usize>) -> Vec<String> {
    let mut v: Vec<(&String, &usize)> = counts.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v.into_iter().map(|(n, _)| n.clone()).collect()
}

/// F20-FR1 / OQ29: the device's tag vocabulary: the [`VOCABULARY`] most used tags.
pub fn vocabulary<'a>(sidecars: impl IntoIterator<Item = &'a Sidecar>) -> Vec<String> {
    let mut v = by_use(&counts(sidecars));
    v.truncate(VOCABULARY);
    v
}

/// F20-FR1: the summarise call's tags replace the AI's earlier ones. The user's own tags stay,
/// a tag the user removed doesn't come back (F20-AC3), and a reply with no tags leaves the
/// meeting's tags as they were.
pub fn apply_ai(sc: &mut Sidecar, names: &[String]) {
    if names.is_empty() {
        return;
    }
    sc.tags.retain(|t| t.source == TagSource::User);
    for n in names {
        if !sc.tags_user_removed.contains(n) && sc.tags.iter().all(|t| &t.name != n) {
            sc.tags.push(Tag::new(n, TagSource::Ai));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(tags: &[(&str, TagSource)]) -> Sidecar {
        let mut sc = crate::sidecar::blank("meeting-20261007-211706");
        sc.tags = tags.iter().map(|(n, s)| Tag::new(n, *s)).collect();
        sc
    }

    #[test]
    fn ai_tags_replace_the_ai_s_and_keep_the_user_s() {
        let mut sc = with(&[("old", TagSource::Ai), ("mine", TagSource::User)]);
        sc.tags_user_removed = vec!["budget".into()];
        let names: Vec<String> = ["gpu", "budget", "mine"].map(String::from).to_vec();
        apply_ai(&mut sc, &names);
        let got: Vec<(&str, TagSource)> = sc
            .tags
            .iter()
            .map(|t| (t.name.as_str(), t.source))
            .collect();
        assert_eq!(got, [("mine", TagSource::User), ("gpu", TagSource::Ai)]);
        apply_ai(&mut sc, &[]);
        assert_eq!(sc.tags.len(), 2, "a reply without tags changes nothing");
    }

    #[test]
    fn the_vocabulary_is_the_most_used_first() {
        let a = with(&[("gpu", TagSource::Ai), ("vram", TagSource::Ai)]);
        let b = with(&[("gpu", TagSource::User), ("budget", TagSource::Ai)]);
        let c = with(&[("gpu", TagSource::Ai), ("vram", TagSource::User)]);
        assert_eq!(vocabulary([&a, &b, &c]), ["gpu", "vram", "budget"]);
        let many: Vec<Sidecar> = (0..250)
            .map(|i| with(&[(&format!("t{i:03}"), TagSource::Ai)]))
            .collect();
        let v = vocabulary(&many);
        assert_eq!(v.len(), VOCABULARY);
        assert_eq!(v[0], "t000");
    }

    #[test]
    fn tags_are_normalised() {
        let cases = [
            ("#Local AI", Some("local-ai")),
            ("GPU", Some("gpu")),
            ("rtx_spark", Some("rtx-spark")),
            ("  ##vram  ", Some("vram")),
            ("v1.2 release!", Some("v1-2-release")),
            ("Ana's notes", Some("anas-notes")),
            ("--a--b--", Some("a-b")),
            ("Über Straße", Some("über-straße")),
            ("東京 会議", Some("東京-会議")),
            ("#", None),
            ("  ", None),
            ("!!!", None),
            ("😀", None),
        ];
        for (input, want) in cases {
            assert_eq!(normalise(input).as_deref(), want, "{input}");
        }
        assert_eq!(shown("local-ai"), "#local-ai");
    }

    #[test]
    fn long_tags_are_cut_without_a_trailing_dash() {
        let long = format!("{} {}", "a".repeat(31), "b".repeat(10));
        let n = normalise(&long).unwrap();
        assert_eq!(n, "a".repeat(31));
        let n = normalise(&"é".repeat(40)).unwrap();
        assert_eq!(n.chars().count(), MAX_TAG_CHARS);
    }

    #[test]
    fn duplicates_within_a_meeting_are_dropped() {
        let got = normalise_all(["GPU", "#gpu", "Local AI", "local-ai", "", "vram"]);
        assert_eq!(got, ["gpu", "local-ai", "vram"]);
    }
}
