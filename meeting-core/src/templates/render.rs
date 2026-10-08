//! F11-FR4: rendering a layout with minijinja. Templates are untrusted text: the engine has no
//! loader (no filesystem or network), runs on a fuel budget with a recursion limit, and writes
//! into a capped buffer. Constructs that build values in memory are refused before compiling
//! (`guard`). The environment starts empty: only the filters and tests listed here exist, each
//! refusing an input or result past [`MAX_VALUE_BYTES`] / [`MAX_VALUE_ITEMS`] (F11-FR7). A
//! layout only reads the context built here from the sidecar.
use minijinja::value::{Rest, ValueKind};
use minijinja::{
    AutoEscape, Environment, Error, ErrorKind, UndefinedBehavior, Value, escape_formatter,
};
use serde::Serialize;
use std::io;

/// Enough for a 3-hour transcript through the standard layout many times over; stops a runaway
/// loop in a hostile layout.
pub const FUEL: u64 = 2_000_000;
pub const RECURSION_LIMIT: usize = 64;
pub const MAX_OUTPUT: usize = 2 * 1024 * 1024;
/// `join`'s separator: long enough for ", " or " · ", too short to multiply a list by much.
pub const MAX_JOIN_SEPARATOR: usize = 16;
/// The largest string a filter takes or returns.
pub const MAX_VALUE_BYTES: usize = 256 * 1024;
/// The longest list a filter takes or returns.
pub const MAX_VALUE_ITEMS: usize = 10_000;

/// The output, refusing to grow past [`MAX_OUTPUT`].
#[derive(Default)]
struct Capped {
    buf: Vec<u8>,
    overflowed: bool,
}

impl io::Write for Capped {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.buf.len() + b.len() > MAX_OUTPUT {
            self.overflowed = true;
            return Err(io::Error::other("output too large"));
        }
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn invalid(what: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidOperation, what.into())
}

/// `v` if it is within the limits, else an error naming `filter`.
fn sized(filter: &str, v: Value) -> Result<Value, Error> {
    let too_big = match v.kind() {
        ValueKind::String => v.as_str().is_some_and(|s| s.len() > MAX_VALUE_BYTES),
        ValueKind::Seq | ValueKind::Map | ValueKind::Iterable => {
            v.len().is_none_or(|n| n > MAX_VALUE_ITEMS)
        }
        _ => false,
    };
    if too_big {
        return Err(invalid(format!("{filter}: the value is too large")));
    }
    Ok(v)
}

/// The string a string filter was given, or an error.
fn text<'a>(filter: &str, v: &'a Value) -> Result<&'a str, Error> {
    match v.as_str() {
        Some(s) if s.len() <= MAX_VALUE_BYTES => Ok(s),
        Some(_) => Err(invalid(format!("{filter}: the value is too large"))),
        None => Err(invalid(format!("{filter} takes text, not {}", v.kind()))),
    }
}

/// Registers a string → string filter.
fn string_filter(e: &mut Environment<'static>, name: &'static str, f: fn(&str) -> String) {
    e.add_filter(name, move |v: Value| -> Result<Value, Error> {
        sized(name, Value::from(f(text(name, &v)?)))
    });
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first
            .to_uppercase()
            .chain(c.as_str().to_lowercase().chars())
            .collect(),
        None => String::new(),
    }
}

fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = !c.is_alphanumeric();
    }
    out
}

/// `default(other="", lax=false)`.
fn default(v: Value, args: Rest<Value>) -> Result<Value, Error> {
    if args.len() > 2 {
        return Err(Error::from(ErrorKind::TooManyArguments));
    }
    let v = sized("default", v)?;
    let other = args.first().cloned().unwrap_or_else(|| Value::from(""));
    let lax = args.get(1).is_some_and(Value::is_true);
    if v.is_undefined() || (lax && !v.is_true()) {
        sized("default", other)
    } else {
        Ok(v)
    }
}

fn length(v: Value) -> Result<Value, Error> {
    let v = sized("length", v)?;
    match v.as_str() {
        Some(s) => Ok(Value::from(s.chars().count())),
        None => v
            .len()
            .map(Value::from)
            .ok_or_else(|| invalid(format!("length of {} is unknown", v.kind()))),
    }
}

/// `first` / `last` of a string (a character) or a list (an item).
fn end_of(name: &'static str, v: Value, last: bool) -> Result<Value, Error> {
    let v = sized(name, v)?;
    if let Some(s) = v.as_str() {
        let c = if last {
            s.chars().next_back()
        } else {
            s.chars().next()
        };
        return Ok(c.map_or(Value::UNDEFINED, Value::from));
    }
    match v.kind() {
        ValueKind::Seq => {
            let n = v.len().unwrap_or(0);
            if n == 0 {
                return Ok(Value::UNDEFINED);
            }
            let i = if last { n - 1 } else { 0 };
            v.get_item_by_index(i)
        }
        k => Err(invalid(format!("{name} takes text or a list, not {k}"))),
    }
}

/// `join(sep="")`: a list of text and numbers, with a short separator, into text of at most
/// [`MAX_VALUE_BYTES`]. A string is printed as it is, not joined per character.
fn join(v: Value, sep: Option<String>) -> Result<Value, Error> {
    let sep = sep.unwrap_or_default();
    if sep.chars().count() > MAX_JOIN_SEPARATOR {
        let e = format!("join's separator is limited to {MAX_JOIN_SEPARATOR} characters");
        return Err(invalid(e));
    }
    let v = sized("join", v)?;
    if v.as_str().is_some() {
        return Ok(v);
    }
    if v.kind() != ValueKind::Seq {
        return Err(invalid(format!("join takes a list, not {}", v.kind())));
    }
    let mut out = String::new();
    for (n, item) in v.try_iter()?.enumerate() {
        if n > 0 {
            out.push_str(&sep);
        }
        match item.kind() {
            ValueKind::String => out.push_str(item.as_str().unwrap_or_default()),
            ValueKind::Number => out.push_str(&item.to_string()),
            k => return Err(invalid(format!("join takes text and numbers, not {k}"))),
        }
        if out.len() > MAX_VALUE_BYTES {
            return Err(invalid("join: the value is too large"));
        }
    }
    Ok(Value::from(out))
}

/// A render failure. `line` is 1-based in the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderError {
    pub message: String,
    pub line: Option<usize>,
}

/// The environment: empty, then only what a layout may use.
fn env() -> Environment<'static> {
    let mut e = Environment::empty();
    e.set_auto_escape_callback(|_| AutoEscape::None); // Markdown, not HTML
    e.set_undefined_behavior(UndefinedBehavior::Chainable); // a missing variable is empty
    e.set_fuel(Some(FUEL));
    e.set_recursion_limit(RECURSION_LIMIT);
    e.set_keep_trailing_newline(true);
    e.add_filter("default", default);
    string_filter(&mut e, "upper", str::to_uppercase);
    string_filter(&mut e, "lower", str::to_lowercase);
    string_filter(&mut e, "title", title);
    string_filter(&mut e, "capitalize", capitalize);
    string_filter(&mut e, "trim", |s| s.trim().to_owned());
    e.add_filter("length", length);
    e.add_filter("first", |v: Value| end_of("first", v, false));
    e.add_filter("last", |v: Value| end_of("last", v, true));
    e.add_filter("join", join);
    // Tests only answer yes or no.
    e.add_test("defined", |v: Value| !v.is_undefined());
    e.add_test("undefined", |v: Value| v.is_undefined());
    e.add_test("none", |v: Value| v.is_none());
    e.add_test("string", |v: Value| v.kind() == ValueKind::String);
    e.add_test("number", |v: Value| v.kind() == ValueKind::Number);
    e.add_test("sequence", |v: Value| v.kind() == ValueKind::Seq);
    e.add_test("mapping", |v: Value| v.kind() == ValueKind::Map);
    e.add_test("true", |v: Value| {
        v.kind() == ValueKind::Bool && v.is_true()
    });
    e.add_test("false", |v: Value| {
        v.kind() == ValueKind::Bool && !v.is_true()
    });
    // A null value (an untitled meeting, a to-do with no owner) prints as nothing, like a
    // missing one, rather than "none".
    e.set_formatter(|out, state, v| {
        let v = if v.is_none() { &Value::UNDEFINED } else { v };
        escape_formatter(out, state, v)
    });
    e
}

/// Render `layout` with `ctx`. An error reads "Template error on line N: …".
pub fn render(layout: &str, ctx: &Context) -> Result<String, RenderError> {
    let e = env();
    let to_err = |x: minijinja::Error| {
        let line = x.line();
        let what = x
            .detail()
            .map_or_else(|| x.kind().to_string(), str::to_owned);
        let message = match line {
            Some(l) => format!("Template error on line {l}: {what}"),
            None => format!("Template error: {what}"),
        };
        RenderError {
            message: crate::scrub::scrub(&message, &[]),
            line,
        }
    };
    super::guard::check(layout)?;
    let mut out = Capped::default();
    let r = e
        .template_from_str(layout)
        .and_then(|t| t.render_captured_to(ctx, &mut out).map(|_| ()));
    if out.overflowed {
        return Err(RenderError {
            message: "Template error: the output is larger than 2 MB".into(),
            line: None,
        });
    }
    r.map_err(to_err)?;
    String::from_utf8(out.buf).map_err(|_| RenderError {
        message: "Template error: the output is not text".into(),
        line: None,
    })
}

/// The layout variables (F11-FR4), built in code from the sidecar.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Context {
    pub title: Option<String>,
    pub date: String,
    pub weekday: String,
    pub time: String,
    pub duration: String,
    pub mic: String,
    /// The template's name.
    pub template: String,
    pub providers: CtxProviders,
    pub summary: String,
    pub decisions: Vec<String>,
    pub todos: Vec<CtxTodo>,
    pub sections: Vec<CtxSection>,
    pub transcript: Vec<CtxPara>,
}

/// What ran, as "openai gpt-transcribe"; empty when nothing did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CtxProviders {
    pub transcribe: String,
    pub summarise: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtxTodo {
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtxSection {
    pub heading: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtxPara {
    pub marker: String,
    pub text: String,
}

/// "47 min", "1 h 05 min".
pub fn duration_label(ms: u64) -> String {
    let m = ms / 60_000;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

/// The sample meeting a layout must render before it is used or saved, and that Preview shows
/// (F11-FR5). Every variable has a value.
pub fn sample_context() -> Context {
    let para = |ms: u64, text: &str| CtxPara {
        marker: crate::pipeline::marker(ms),
        text: text.into(),
    };
    Context {
        title: Some("Sample: weekly planning".into()),
        date: "2026-10-07".into(),
        weekday: "Wednesday".into(),
        time: "21:17".into(),
        duration: duration_label(1_920_000),
        mic: "USB — USB-Audio".into(),
        template: "Sample".into(),
        providers: CtxProviders {
            transcribe: "openai gpt-transcribe".into(),
            summarise: "anthropic claude-opus-5-5".into(),
        },
        summary: "The team agreed the Q4 scope and who sends the deck.".into(),
        decisions: vec!["Ship the reader before the editor".into()],
        todos: vec![
            CtxTodo {
                text: "Send the deck to Priya".into(),
                owner: Some("me".into()),
                due: Some("2026-10-09".into()),
                done: false,
            },
            CtxTodo {
                text: "Book the review room".into(),
                owner: None,
                due: None,
                done: true,
            },
        ],
        sections: vec![CtxSection {
            heading: "Blockers".into(),
            body: "- None raised.".into(),
        }],
        transcript: vec![
            para(0, "Let's start with the plan."),
            para(900_000, "I'll send the deck to Priya by Friday."),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration_label(0), "0 min");
        assert_eq!(duration_label(2_820_000), "47 min");
        assert_eq!(duration_label(3_900_000), "1 h 05 min");
    }

    fn r(layout: &str) -> Result<String, String> {
        render(layout, &sample_context()).map_err(|e| e.message)
    }

    #[test]
    fn only_the_allowed_filters_exist() {
        let ok = [
            (
                "{{ nope|default('x') }}|{{ ''|default('y', true) }}|{{ 'a'|default }}",
                "x|y|a",
            ),
            (
                "{{ 'aB c'|upper }}{{ 'aB'|lower }}{{ 'hi there-you'|title }}",
                "AB CabHi There-You",
            ),
            (
                "{{ 'hELLO'|capitalize }}|{{ ''|capitalize }}|{{ '  x '|trim }}",
                "Hello||x",
            ),
            (
                "{{ 'héllo'|length }}{{ todos|length }}{{ 'ab'|first }}{{ 'ab'|last }}",
                "52ab",
            ),
            (
                "{{ (todos|first).text }}|{{ (todos|last).done }}|{{ ''|first }}",
                "Send the deck to Priya|True|",
            ),
            (
                "{{ decisions|join(', ') }}|{{ 'ab'|join(',') }}|{{ todos|map|length }}",
                "",
            ),
            (
                "{{ summary is defined }}{{ nope is undefined }}{{ title is none }}",
                "TrueTrueFalse",
            ),
            (
                "{{ 'a' is string }}{{ 1 is number }}{{ todos is sequence }}{{ providers is mapping }}",
                "TrueTrueTrueTrue",
            ),
            ("{{ true is true }}{{ 1 is false }}", "TrueFalse"),
        ];
        for (l, want) in ok {
            let got = r(l);
            if want.is_empty() {
                assert!(got.is_err(), "{l}: map is not allowed");
            } else {
                assert_eq!(got.as_deref(), Ok(want), "{l}");
            }
        }
        for bad in [
            "{{ summary|pprint }}",
            "{{ summary|replace('a', 'b') }}",
            "{{ todos|sort }}",
            "{{ summary|e }}",
            "{{ summary|string }}",
            "{{ range(3) }}",
            "{{ summary is odd }}",
            "{{ todos|upper }}",
            "{{ 1|trim }}",
            "{{ 1|first }}",
            "{{ providers|length }}{{ 1|length }}",
            "{{ todos|join(',') }}",
            "{{ 'ab'|default(1, 2, 3) }}",
            "{{ decisions|join('12345678901234567') }}",
            "{{ summary|join }}{{ providers|join }}",
        ] {
            assert!(r(bad).is_err(), "{bad}");
        }
        assert_eq!(
            r("{{ decisions|join(' · ') }}").unwrap(),
            "Ship the reader before the editor"
        );
    }

    #[test]
    fn filters_refuse_large_values() {
        let big = Value::from("x".repeat(MAX_VALUE_BYTES + 1));
        assert!(sized("f", big.clone()).is_err());
        assert!(text("f", &big).is_err());
        assert!(join(big.clone(), None).is_err());
        assert!(sized("f", Value::from(vec![0; MAX_VALUE_ITEMS + 1])).is_err());
        let half = Value::from(vec!["x".repeat(MAX_VALUE_BYTES / 2 + 1); 2]);
        assert!(join(half, None).is_err());
        assert!(sized("f", Value::from(vec![1, 2])).is_ok());
        assert!(
            join(Value::from(vec![1, 2]), Some(",".into()))
                .unwrap()
                .as_str()
                == Some("1,2")
        );
    }

    #[test]
    fn errors_carry_the_line_and_missing_variables_are_empty() {
        let ctx = Context::default();
        assert_eq!(render("a{{ nope.deeper }}b\n", &ctx).unwrap(), "ab\n");
        let e = render("x\n{{ 1 + }}\n", &ctx).unwrap_err();
        assert_eq!(e.line, Some(2));
        assert!(
            e.message.starts_with("Template error on line 2: "),
            "{}",
            e.message
        );
    }
}
