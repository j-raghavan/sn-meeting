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

/// The shortest run of a key, from its start or its end, that is redacted on its own.
const MIN_RUN: usize = 12;
/// Any run of this many characters from anywhere inside a key is redacted too.
const MID_RUN: usize = 16;

/// Scrub `text`: every `secret` occurrence (also percent-encoded, any run of at least 12 of its
/// first or last characters, and any 16 characters from inside it), then the values of the auth headers (as `name: v`, `name=v`
/// or JSON `"name": "v"`), bearer tokens and `key=` values; then truncate. Idempotent.
/// Every non-empty secret is replaced whatever its length: callers pass key values only.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        for form in [s.to_string(), percent(s, true), percent(s, false)] {
            out = out.replace(&form, REDACTED);
        }
        out = redact_runs(&out, s);
    }
    for name in HEADER_NAMES {
        // A bare "Authorization: failed" is prose; a credential looks like a token.
        let rule = Rule {
            token: name == "authorization",
            boundary: false,
        };
        out = redact_after(&out, name, &[':', '='], rule);
    }
    let bearer = Rule {
        token: true,
        boundary: false,
    };
    out = redact_after(&out, "bearer", &[], bearer);
    // "monkey=banana" is not a key; "?key=", "&key=", "api_key=" and "key =" are.
    let key = Rule {
        token: false,
        boundary: true,
    };
    out = redact_after(&out, "key", &['='], key);
    truncate(&out, MAX_LEN)
}

/// Whether `text` holds key material by [`scrub`]'s rule: a whole secret (also
/// percent-encoded), a run of at least 12 of its first or last characters, or any 16 from
/// inside it.
pub fn contains_secret(text: &str, secrets: &[&str]) -> bool {
    secrets.iter().filter(|s| !s.is_empty()).any(|s| {
        [s.to_string(), percent(s, true), percent(s, false)]
            .iter()
            .any(|f| text.contains(f.as_str()))
            || runs(s).any(|r| text.contains(&r))
    })
}

/// Every run of `secret`'s first or last characters of at least [`MIN_RUN`], longest first,
/// then every [`MID_RUN`]-character window from inside it.
fn runs(secret: &str) -> impl Iterator<Item = String> + '_ {
    let n = secret.chars().count();
    let ends = (MIN_RUN..n).rev().flat_map(move |len| {
        let head: String = secret.chars().take(len).collect();
        let tail: String = secret.chars().skip(n - len).collect();
        [head, tail]
    });
    let windows = (1..n.saturating_sub(MID_RUN))
        .map(move |start| secret.chars().skip(start).take(MID_RUN).collect::<String>());
    ends.chain(windows)
}

/// `s` percent-encoded (every byte outside the URL-unreserved set), in upper or lower hex.
fn percent(s: &str, upper: bool) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ if upper => format!("%{b:02X}"),
            _ => format!("%{b:02x}"),
        })
        .collect()
}

/// Replace every run of `secret`'s first or last `MIN_RUN`+ characters (a key cut short by a
/// log line or a provider's echo), longest first, then any `MID_RUN` from inside it.
fn redact_runs(text: &str, secret: &str) -> String {
    let mut out = text.to_owned();
    for run in runs(secret) {
        if out.contains(&run) {
            out = out.replace(&run, REDACTED);
        }
    }
    out
}

/// What a marker's value must satisfy to be redacted.
#[derive(Clone, Copy)]
struct Rule {
    /// The value must look like a credential (not a plain word such as "of" or "failed").
    token: bool,
    /// The marker must start a word (`key=`, but not `monkey=`).
    boundary: bool,
}

/// A credential-like token: credential characters only, and a digit, a symbol, an inner
/// capital or at least 20 characters ("sk-raw", "xyz123", "dXNlcjpw", not "of" or "Failed").
fn token_like(v: &str) -> bool {
    let charset = v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._~+/=".contains(c));
    let marked = v
        .chars()
        .any(|c| c.is_ascii_digit() || "-._~+/=".contains(c))
        || v.chars().skip(1).any(|c| c.is_ascii_uppercase())
        || v.len() >= 20;
    !v.is_empty() && charset && marked
}

/// Replace the value after each case-insensitive `marker`. With `seps`, the marker must be
/// followed (after optional quotes and spaces) by one of them; without, by whitespace.
fn redact_after(text: &str, marker: &str, seps: &[char], rule: Rule) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut done = 0; // bytes of `text` already copied to `out`
    let mut from = 0;
    while let Some(off) = lower[from..].find(marker) {
        let start = from + off;
        let mut i = start + marker.len();
        from = i;
        if rule.boundary
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
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
        let mut schemed = false;
        // "Authorization: Bearer <token>": keep the scheme, redact the credential.
        if SCHEMES.contains(&lower[i..end].as_ref()) {
            let next = skip(text, end, |c| c == ' ' || c == '\t');
            if next > end {
                i = next;
                end = token_end(text, i);
                schemed = true;
            }
        }
        if rule.token && !schemed && !token_like(&text[i..end]) && !text[i..].starts_with(REDACTED)
        {
            continue;
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
