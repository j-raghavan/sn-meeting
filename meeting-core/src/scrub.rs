//! F4-FR6: remove key material from any text before it leaves the core (INV2).

const REDACTED: &str = "[redacted]";
/// Longest text that leaves the core, counting the "…" that marks a cut.
pub const MAX_LEN: usize = 300;
/// How far a cut may back off to reach a delimiter, so it never splits a value.
const MAX_BACKOFF: usize = 64;
const ELLIPSIS: &str = "…";
const HEADER_NAMES: [&str; 3] = ["authorization", "x-api-key", "x-goog-api-key"];
/// Auth schemes kept visible before a redacted credential ("Bearer [redacted]").
const SCHEMES: [&str; 3] = ["bearer", "basic", "token"];

/// Scrub `text`: every `secret` occurrence, then the values of the auth headers (as `name: v`,
/// `name=v` or JSON `"name": "v"`), bearer tokens and `key=` values; then truncate. Idempotent.
/// Every non-empty secret is replaced whatever its length: callers pass key values only.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        out = out.replace(*s, REDACTED);
    }
    for name in HEADER_NAMES {
        out = redact_after(&out, name, &[':', '=']);
    }
    out = redact_after(&out, "bearer", &[]);
    out = redact_after(&out, "key", &['=']);
    truncate(&out, MAX_LEN)
}

/// Replace the value after each case-insensitive `marker`. With `seps`, the marker must be
/// followed (after optional quotes and spaces) by one of them; without, by whitespace.
fn redact_after(text: &str, marker: &str, seps: &[char]) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut done = 0; // bytes of `text` already copied to `out`
    let mut from = 0;
    while let Some(off) = lower[from..].find(marker) {
        let mut i = from + off + marker.len();
        from = i;
        if seps.is_empty() {
            let after = skip(text, i, |c| c == ' ' || c == '\t');
            if after == i {
                continue; // "bearerx": not a scheme
            }
            i = after;
        } else {
            i = skip(text, i, |c| c == '"' || c == '\'' || c == ' ' || c == '\t');
            match text[i..].chars().next() {
                Some(c) if seps.contains(&c) => i += c.len_utf8(),
                _ => continue,
            }
            i = skip(text, i, |c| c == '"' || c == '\'' || c == ' ' || c == '\t');
        }
        let mut end = token_end(text, i);
        // "Authorization: Bearer <token>": keep the scheme, redact the credential.
        if SCHEMES.contains(&lower[i..end].as_ref()) {
            let next = skip(text, end, |c| c == ' ' || c == '\t');
            if next > end {
                i = next;
                end = token_end(text, i);
            }
        }
        if text[i..].starts_with(REDACTED) {
            from = i + REDACTED.len(); // already scrubbed
        } else if &text[i..end] == ELLIPSIS {
            from = end; // our own cut mark, not a value
        } else if end > i {
            out.push_str(&text[done..i]);
            out.push_str(REDACTED);
            done = end;
        }
        from = from.max(end);
    }
    out.push_str(&text[done..]);
    out
}

/// The first byte at or after `i` whose char does not satisfy `f`.
fn skip(text: &str, i: usize, f: impl Fn(char) -> bool) -> usize {
    text[i..]
        .find(|c: char| !f(c))
        .map_or(text.len(), |n| i + n)
}

const STOPS: &[char] = &['"', '\'', ',', ';', '&', ')', '}', ']', '<', '>'];

/// The end of the credential-like token starting at `i`.
fn token_end(text: &str, i: usize) -> usize {
    skip(text, i, |c| !c.is_whitespace() && !STOPS.contains(&c))
}

/// Cut to at most `max` chars including the "…". The cut backs off to just after a delimiter
/// (whitespace, a stop char, `:` or `=`) so no part of a value token survives; scrubbing the
/// result again then changes nothing.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let hard = s.char_indices().nth(max - 1).map_or(s.len(), |(i, _)| i);
    let head = &s[..hard];
    let cut = head
        .char_indices()
        .rev()
        .take(MAX_BACKOFF)
        .find(|&(_, c)| is_delimiter(c))
        .map_or(hard, |(i, c)| i + c.len_utf8());
    format!("{}{ELLIPSIS}", &s[..cut])
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || STOPS.contains(&c) || c == ':' || c == '='
}
