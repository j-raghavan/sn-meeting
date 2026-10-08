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
    // ordinary key=value pairs and prose that merely names a scheme or header
    ("monkey=banana", "monkey=banana"),
    ("hotkey=F1, turnkey = yes", "hotkey=F1, turnkey = yes"),
    ("api_key=sk-1&x=2", "api_key=[redacted]&x=2"),
    ("Bearer of bad news", "Bearer of bad news"),
    ("Authorization: failed for project", "Authorization: failed for project"),
    ("authorization: Required", "authorization: Required"),
    ("bearer dXNlcjpwYXNz rest", "bearer [redacted] rest"),
    // the known secret cut short, or URL-encoded
    ("echo sk-proj-SECR!", "echo [redacted]!"),
    ("tail ...ECRET-0123456789", "tail ...[redacted]"),
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

/// Every cut position near MAX_LEN, for each way a value can follow a marker: scrubbing twice
/// equals scrubbing once, nothing exceeds MAX_LEN, and no piece of the unknown value survives.
#[test]
fn idempotent_at_every_cut() {
    let templates = [
        "x-api-key: sk-unknown-VALUE-123 tail",
        "Authorization: Bearer sk-unknown-VALUE-123 tail",
        "authorization:Bearer sk-unknown-VALUE-123,tail",
        "GET /m?pageSize=1&key=sk-unknown-VALUE-123&x=1",
        r#"{"x-goog-api-key": "sk-unknown-VALUE-123"}"#,
        "bearer sk-unknown-VALUE-123",
    ];
    for t in templates {
        for at in MAX_LEN - 40..=MAX_LEN {
            for pad in ["a", "a b "] {
                let mut text = pad.repeat(at).chars().take(at).collect::<String>();
                text.push_str(t);
                text.push_str(&" z".repeat(20));
                let once = scrub(&text, &[]);
                assert_eq!(scrub(&once, &[]), once, "{t:?} at {at} pad {pad:?}");
                assert!(once.chars().count() <= MAX_LEN, "{t:?} at {at}");
                assert!(!once.contains("sk-u") && !once.contains("VALUE"), "{once}");
            }
        }
    }
}

#[test]
fn every_nonempty_secret_is_used() {
    assert_eq!(scrub("a cat sat", &[""]), "a cat sat");
    // A short key is still a key: callers pass key values only.
    assert_eq!(scrub("key k9 here", &["k9"]), "key [redacted] here");
}

#[test]
fn truncates_after_redacting() {
    // The secret straddles the cut: it is replaced first, so no prefix of it survives.
    let text = format!("{}{KEY}{}", "a".repeat(MAX_LEN - 5), "b".repeat(50));
    let s = scrub(&text, &[KEY]);
    assert!(!s.contains("sk-proj"), "{s}");
    assert!(s.chars().count() <= MAX_LEN);
    assert!(s.ends_with('…'));
    assert_eq!(scrub(&"z".repeat(MAX_LEN), &[]), "z".repeat(MAX_LEN));
    // A cut inside a word backs off to the space before it.
    let words = "word ".repeat(100);
    let cut = scrub(&words, &[]);
    assert!(
        cut.ends_with(" …") && cut.chars().count() <= MAX_LEN,
        "{cut}"
    );
    // With no delimiter in reach, the cut is hard.
    let blob = "x".repeat(MAX_LEN * 2);
    assert_eq!(scrub(&blob, &[]), format!("{}…", "x".repeat(MAX_LEN - 1)));
}

#[test]
fn encoded_and_partial_keys() {
    let key = "sk-ant+SECRET/x=0002abcdef";
    assert_eq!(
        scrub("v=sk-ant%2BSECRET%2Fx%3D0002abcdef;", &[key]),
        "v=[redacted];"
    );
    assert_eq!(
        scrub("v=sk-ant%2bSECRET%2fx%3d0002abcdef;", &[key]),
        "v=[redacted];"
    );
    // Runs shorter than 12 characters are left: they are not the key.
    assert_eq!(scrub("sk-ant+SECR", &[key]), "sk-ant+SECR");
    assert_eq!(scrub("sk-ant+SECRE", &[key]), "[redacted]");
    // A key of 12 characters or fewer is only replaced whole.
    assert_eq!(
        scrub("abcdefghijkl abcdefghijk", &["abcdefghijkl"]),
        "[redacted] abcdefghijk"
    );
}

#[test]
fn a_window_from_the_middle_of_a_key_is_redacted() {
    // Review 3 #5: 16 characters from inside a key, with neither end of it.
    let key = "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
    let middle = &key[10..26];
    assert_eq!(middle.len(), 16);
    let out = scrub(&format!("echo: {middle} end"), &[key]);
    assert_eq!(out, "echo: [redacted] end");
    assert!(meeting::scrub::contains_secret(
        &format!("x{middle}y"),
        &[key]
    ));
    // 15 characters from the middle are not enough to count as the key.
    let short = &key[10..25];
    assert_eq!(scrub(short, &[key]), short);
}
