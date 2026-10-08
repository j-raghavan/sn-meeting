//! F11-FR7: what a layout may do, checked on the source before it is compiled. Fuel bounds
//! the steps a render takes but not what one step allocates, so the constructs that build large
//! values in memory are refused outright: assignment and capture (`set`, `macro`, `call`,
//! `filter`, `with`), and string repetition and concatenation (`*`, `~`, `+`). What a layout
//! can then print is bounded by the context and the source, and the output writer is capped.
use super::render::RenderError;

/// Statements a layout may use; `raw` blocks are skipped whole.
const ALLOWED: [&str; 6] = ["if", "elif", "else", "endif", "for", "endfor"];

/// Words after which a `[` starts a list literal rather than a subscript.
const KEYWORDS: [&str; 9] = ["in", "if", "else", "and", "or", "not", "is", "elif", "for"];

fn refuse(line: usize, what: &str) -> RenderError {
    RenderError {
        message: format!("Template error on line {line}: {what} isn't allowed in a layout"),
        line: Some(line),
    }
}

/// `Ok` when `layout` uses only what a layout may; else the first refused construct, by line.
pub fn check(layout: &str) -> Result<(), RenderError> {
    let s = layout.as_bytes();
    let (mut i, mut line) = (0, 1);
    while i < s.len() {
        let open = |c: u8| s[i] == b'{' && s.get(i + 1) == Some(&c);
        if open(b'#') {
            i = skip_past(s, i + 2, b"#}", &mut line);
        } else if open(b'{') {
            i = tag(s, i + 2, b"}}", &mut line, false)?;
        } else if open(b'%') {
            i = tag(s, i + 2, b"%}", &mut line, true)?;
        } else {
            line += usize::from(s[i] == b'\n');
            i += 1;
        }
    }
    Ok(())
}

/// Index just past `end` from `i` (or the end of input), counting lines.
fn skip_past(s: &[u8], mut i: usize, end: &[u8], line: &mut usize) -> usize {
    while i < s.len() && !s[i..].starts_with(end) {
        *line += usize::from(s[i] == b'\n');
        i += 1;
    }
    (i + end.len()).min(s.len())
}

/// Checks one `{{ … }}` or `{% … %}` body from `i`; returns the index past its close.
fn tag(
    s: &[u8],
    mut i: usize,
    close: &[u8],
    line: &mut usize,
    stmt: bool,
) -> Result<usize, RenderError> {
    let mut first_word = stmt;
    // Whether the last token was a value (a name, number, string or closing bracket): a `[`
    // after one is a subscript, anywhere else it starts a list literal.
    let mut after_value = false;
    while i < s.len() {
        let c = s[i];
        if s[i..].starts_with(close) {
            return Ok(i + close.len());
        }
        match c {
            b'\n' => *line += 1,
            c if c.is_ascii_whitespace() => {}
            b'\'' | b'"' => {
                // A string literal, escapes included: its contents are data, not operators.
                i += 1;
                while i < s.len() && s[i] != c {
                    *line += usize::from(s[i] == b'\n');
                    i += if s[i] == b'\\' { 2 } else { 1 };
                }
                after_value = true;
            }
            b'~' => return Err(refuse(*line, "The ~ operator")),
            b'+' => return Err(refuse(*line, "The + operator")),
            b'*' => return Err(refuse(*line, "The * operator")),
            b'{' => return Err(refuse(*line, "A { } literal")),
            b'[' if !after_value => return Err(refuse(*line, "A [ ] literal")),
            b')' | b']' => after_value = true,
            c if first_word && (c.is_ascii_alphabetic() || c == b'_') => {
                let end = (i..s.len())
                    .find(|&j| !(s[j].is_ascii_alphanumeric() || s[j] == b'_'))
                    .unwrap_or(s.len());
                let word = std::str::from_utf8(&s[i..end]).unwrap_or_default();
                if word == "raw" {
                    let past = skip_past(s, end, close, line);
                    return Ok(skip_raw(s, past, line));
                }
                if !ALLOWED.contains(&word) {
                    return Err(refuse(*line, &format!("{{% {word} %}}")));
                }
                first_word = false;
                i = end;
                continue;
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                let end = (i..s.len())
                    .find(|&j| !(s[j].is_ascii_alphanumeric() || s[j] == b'_'))
                    .unwrap_or(s.len());
                let word = std::str::from_utf8(&s[i..end]).unwrap_or_default();
                after_value = !KEYWORDS.contains(&word);
                i = end;
                continue;
            }
            _ => after_value = false,
        }
        i += 1;
    }
    Ok(s.len())
}

/// Past the `{% endraw %}` that closes a raw block (its body is literal text).
fn skip_raw(s: &[u8], mut i: usize, line: &mut usize) -> usize {
    while i < s.len() {
        if s[i..].starts_with(b"{%") {
            let body_end = (i + 2..s.len()).find(|&j| s[j..].starts_with(b"%}"));
            if let Some(j) = body_end {
                let body = String::from_utf8_lossy(&s[i + 2..j]);
                if body.trim_matches(|c: char| c == '-' || c == '+' || c.is_whitespace())
                    == "endraw"
                {
                    return j + 2;
                }
            }
        }
        *line += usize::from(s[i] == b'\n');
        i += 1;
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_of(src: &str) -> Option<usize> {
        check(src).err().and_then(|e| e.line)
    }

    #[test]
    fn ordinary_layouts_pass() {
        let ok = "# {{ title or \"Untitled\" }}\n{% for t in todos %}- [{{ \"x\" if t.done else \" \" }}] \
                  {{ t.text }}{% if t.owner %} — {{ t.owner }}{% endif %}\n{% endfor %}{# a + b * c ~ d #}\
                  {{ '~ + *' }}{{ \"a\\\"~\" }}{%- if x -%}{%- else -%}{%- endif -%}{{ loop.index - 1 }}";
        assert_eq!(check(ok), Ok(()));
        assert_eq!(
            check("{% raw %}{{ a ~ b }}{% set x %}{% endraw %}{{ c }}"),
            Ok(())
        );
        assert_eq!(check("{%- raw -%}\n{%- endraw -%}"), Ok(()));
        assert_eq!(check("{{ unclosed"), Ok(())); // the compiler reports it
    }

    #[test]
    fn refused_constructs_carry_their_line() {
        assert_eq!(line_of("a\n\n{% set a = 1 %}"), Some(3));
        assert_eq!(line_of("{{ x\n ~ y }}"), Some(2));
        assert_eq!(line_of("{{ 'a' * 3 }}"), Some(1));
        assert_eq!(line_of("{{ 1 + 2 }}"), Some(1));
        assert_eq!(line_of("{% macro m() %}{% endmacro %}"), Some(1));
        assert_eq!(line_of("{%- filter upper %}x{% endfilter %}"), Some(1));
        assert_eq!(line_of("{% for i in x %}{{ i * 2 }}{% endfor %}"), Some(1));
        assert_eq!(line_of("{% raw %}x"), None); // unterminated raw: the compiler reports it
        let e = check("{% with a = 1 %}{% endwith %}").unwrap_err();
        assert_eq!(
            e.message,
            "Template error on line 1: {% with %} isn't allowed in a layout"
        );
        assert_eq!(line_of("{{ [summary]|length }}"), Some(1));
        assert_eq!(line_of("{{ x\n|default([1]) }}"), Some(2));
        assert_eq!(line_of("{% for t in [1, 2] %}{% endfor %}"), Some(1));
        assert_eq!(line_of("{{ x if y else [z] }}"), Some(1));
        assert_eq!(line_of("{{ {'k': 1} }}"), Some(1));
        assert_eq!(line_of("{% if x in {'a': 1} %}{% endif %}"), Some(1));
        // Subscripts are not literals.
        assert_eq!(
            check("{{ todos[0].text }}{{ (todos)[0] }}{{ x['k'] }}{{ 'ab'[1] }}"),
            Ok(())
        );
        let e = check("{{ a ~ b }}").unwrap_err();
        assert_eq!(
            e.message,
            "Template error on line 1: The ~ operator isn't allowed in a layout"
        );
    }
}
