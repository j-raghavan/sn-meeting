//! F20: tags. Each meeting gets keyword tags from its Stop call (ADR D20); they are stored in
//! the sidecar, searchable, editable and drawn as the Tag Graph.

/// The longest tag, in characters (F20-FR2).
pub const MAX_TAG_CHARS: usize = 32;

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

#[cfg(test)]
mod tests {
    use super::*;

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
