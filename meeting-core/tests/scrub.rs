//! F4-FR6: the scrubber removes keys and auth header values, keeps the rest readable.
use meeting::scrub::{MAX_LEN, scrub};

const KEY: &str = "sk-proj-SECRET-0123456789";

/// (input, expected) with `KEY` as the known secret.
#[rustfmt::skip]
const CASES: &[(&str, &str)] = &[
    // the known secret, anywhere, every time
    ("bad key sk-proj-SECRET-0123456789!", "bad key [redacted]!"),
    ("sk-proj-SECRET-0123456789/sk-proj-SECRET-0123456789", "[redacted]/[redacted]"),
    // header text, any case, with the scheme kept
    ("Authorization: Bearer abc.def", "Authorization: Bearer [redacted]"),
    ("authorization:Basic dXNlcjpwYXNz, next", "authorization:Basic [redacted], next"),
    ("AUTHORIZATION: sk-raw\nnext line", "AUTHORIZATION: [redacted]\nnext line"),
    ("x-api-key: ak-1\nok", "x-api-key: [redacted]\nok"),
    ("X-Goog-Api-Key: AIza-1 rest", "X-Goog-Api-Key: [redacted] rest"),
    // JSON / Debug forms
    (r#"{"x-api-key": "ak-2", "a": 1}"#, r#"{"x-api-key": "[redacted]", "a": 1}"#),
    (r#"{"authorization":"Bearer t0k"}"#, r#"{"authorization":"Bearer [redacted]"}"#),
    // bearer tokens on their own
    ("token was bearer xyz123 then", "token was bearer [redacted] then"),
    // key= in a query string or a key file echo
    ("GET /v1beta/models?pageSize=1&key=AIzaXYZ&x=1", "GET /v1beta/models?pageSize=1&key=[redacted]&x=1"),
    ("x-goog-api-key=AIza3&y=2", "x-goog-api-key=[redacted]&y=2"),
    ("key = sk-typed", "key = [redacted]"),
    // ordinary prose is left alone
    ("Incorrect API key provided. Check your key.", "Incorrect API key provided. Check your key."),
    ("the bearerless path; authorization required", "the bearerless path; authorization required"),
    ("Authorization: ", "Authorization: "),
    ("", ""),
    // non-ASCII text survives
    ("Fehler – schlüssel: x-api-key: ä-ß", "Fehler – schlüssel: x-api-key: [redacted]"),
];

#[test]
fn table() {
    for (input, want) in CASES {
        assert_eq!(scrub(input, &[KEY]), *want, "input {input:?}");
    }
}

#[test]
fn idempotent() {
    for (input, _) in CASES {
        let once = scrub(input, &[KEY]);
        assert_eq!(scrub(&once, &[KEY]), once, "input {input:?}");
    }
    let long = format!("{} x-api-key: v", "é".repeat(400));
    let once = scrub(&long, &[]);
    assert_eq!(scrub(&once, &[]), once);
}

#[test]
fn tiny_secrets_are_not_used() {
    assert_eq!(scrub("a cat sat", &["a", "", "at"]), "a cat sat");
}

#[test]
fn truncates_after_redacting() {
    // The secret straddles the cut: it is replaced first, so no prefix of it survives.
    let text = format!("{}{KEY}{}", "a".repeat(MAX_LEN - 5), "b".repeat(50));
    let s = scrub(&text, &[KEY]);
    assert!(!s.contains("sk-proj"), "{s}");
    assert_eq!(s.chars().count(), MAX_LEN + 1);
    assert!(s.ends_with('…'));
    assert_eq!(scrub(&"z".repeat(MAX_LEN), &[]), "z".repeat(MAX_LEN));
}
