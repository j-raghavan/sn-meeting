//! F11-FR4: rendering a layout with minijinja. Templates are untrusted text: the engine has no
//! loader (no filesystem or network), runs on a fuel budget with a recursion limit, and its
//! output is capped. A layout only reads the context built here from the sidecar.
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;

/// Enough for a 3-hour transcript through the standard layout many times over; stops a runaway
/// loop in a hostile layout.
pub const FUEL: u64 = 2_000_000;
pub const RECURSION_LIMIT: usize = 64;
pub const MAX_OUTPUT: usize = 2 * 1024 * 1024;

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
    let out = e.template_from_str(layout).and_then(|t| t.render(ctx));
    let out = out.map_err(to_err)?;
    if out.len() > MAX_OUTPUT {
        return Err(RenderError {
            message: "Template error: the output is larger than 2 MB".into(),
            line: None,
        });
    }
    Ok(out)
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
