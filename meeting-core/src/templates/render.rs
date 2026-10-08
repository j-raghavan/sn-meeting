//! F11-FR4: rendering a layout with minijinja. Templates are untrusted text: the engine has no
//! loader (no filesystem or network), runs on a fuel budget with a recursion limit, and writes
//! into a capped buffer. Constructs that build large values in memory are refused before
//! compiling (`guard`), and so are the filters that multiply their input (F11-FR7). A layout
//! only reads the context built here from the sidecar.
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

/// Built-in filters whose output can be many times their input (`replace`, `indent`, `format`,
/// and the splitters that let `join` pad every character), or that build big lists (`batch`,
/// `slice` take a count). A layout using one fails as an unknown filter.
const REFUSED_FILTERS: [&str; 10] = [
    "replace", "indent", "format", "split", "lines", "list", "batch", "slice", "chain", "zip",
];

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

/// `join` with a short separator. A string is printed as it is, not joined per character.
fn join(v: &Value, sep: Option<&str>) -> Result<String, Error> {
    let sep = sep.unwrap_or("");
    if sep.chars().count() > MAX_JOIN_SEPARATOR {
        let e = format!("join's separator is limited to {MAX_JOIN_SEPARATOR} characters");
        return Err(Error::new(ErrorKind::InvalidOperation, e));
    }
    if let Some(s) = v.as_str() {
        return Ok(s.to_owned());
    }
    let items: Vec<String> = v.try_iter()?.map(|i| i.to_string()).collect();
    Ok(items.join(sep))
}

/// A render failure. `line` is 1-based in the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderError {
    pub message: String,
    pub line: Option<usize>,
}

fn env() -> Environment<'static> {
    let mut e = Environment::new();
    e.set_auto_escape_callback(|_| AutoEscape::None); // Markdown, not HTML
    e.set_undefined_behavior(UndefinedBehavior::Chainable); // a missing variable is empty
    e.set_fuel(Some(FUEL));
    e.set_recursion_limit(RECURSION_LIMIT);
    e.set_keep_trailing_newline(true);
    for f in REFUSED_FILTERS {
        e.remove_filter(f);
    }
    e.add_filter("join", join);
    e.remove_global("namespace");
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
