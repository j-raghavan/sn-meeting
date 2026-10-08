//! F11-FR1 / FR4: the template file format and its validation. Template files are untrusted
//! (they may come from the desktop): every field is length-limited and the layout must compile
//! and render the sample meeting before a template is used or saved.
use super::Template;
use super::render::{render, sample_context};
use serde::Serialize;

pub const MAX_FILE_BYTES: usize = 64 * 1024;
pub const MAX_NAME: usize = 60;
pub const MAX_DESCRIPTION: usize = 200;
pub const MAX_LIST_ITEMS: usize = 50;
pub const MAX_LIST_ITEM: usize = 60;
pub const MAX_INSTRUCTIONS: usize = 8_000;
pub const MAX_LAYOUT: usize = 20_000;

/// A rejected template. `line` is 1-based: in the file for `parse`, in the layout for
/// `validate` (with `field = "layout"`). `field` names the editor field at fault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateError {
    pub error: String,
    pub line: Option<usize>,
    pub field: Option<&'static str>,
}

fn err(
    error: impl Into<String>,
    line: Option<usize>,
    field: Option<&'static str>,
) -> TemplateError {
    TemplateError {
        error: error.into(),
        line,
        field,
    }
}

/// `---` front matter (flat `key: value`, lists as `[a, b]`, optional quotes, unknown keys and
/// `#` lines ignored), then `# Instructions`, then `# Layout`: everything after that line is
/// the layout, verbatim. Validated; errors carry the file line where there is one.
pub fn parse(text: &str) -> Result<Template, TemplateError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(err("The file is larger than 64 KB", None, None));
    }
    // A desktop editor's UTF-8 BOM, and CRLF or old-Mac lone-CR line endings, read as plain lines.
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    if lines[0].trim_end() != "---" {
        let e = "The file must start with a --- line";
        return Err(err(e, Some(1), None));
    }
    let close = lines
        .iter()
        .skip(1)
        .position(|l| l.trim_end() == "---")
        .map(|i| i + 1)
        .ok_or_else(|| err("The front matter has no closing --- line", Some(1), None))?;
    let mut t = Template::empty();
    let mut field_lines: Vec<(&'static str, usize)> = Vec::new();
    for (i, l) in lines[1..close].iter().enumerate() {
        let n = i + 2;
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        let (k, v) = l
            .split_once(':')
            .ok_or_else(|| err(format!("Line {n} should be key: value"), Some(n), None))?;
        let v = unquote(v.trim());
        let field = match k.trim() {
            "id" => {
                t.id = v.to_owned();
                "id"
            }
            "name" => {
                t.name = v.to_owned();
                "name"
            }
            "description" => {
                t.description = v.to_owned();
                "description"
            }
            "vocabulary" => {
                t.vocabulary = list(v, n)?;
                "vocabulary"
            }
            "sections" => {
                t.sections = list(v, n)?;
                "sections"
            }
            _ => continue, // unknown keys are ignored, as unknown sidecar fields are kept
        };
        field_lines.push((field, n));
    }
    let body = &lines[close + 1..];
    let at = |h: &str| body.iter().position(|l| l.trim_end() == h);
    let ins = at("# Instructions")
        .ok_or_else(|| err("Missing the # Instructions heading", None, None))?;
    let lay = at("# Layout").ok_or_else(|| err("Missing the # Layout heading", None, None))?;
    if lay < ins {
        let e = "# Instructions must come before # Layout";
        return Err(err(e, Some(close + 2 + lay), None));
    }
    t.instructions = body[ins + 1..lay].join("\n").trim().to_owned();
    t.layout = body[lay + 1..].join("\n");
    // File line of the layout's first line.
    let layout_start = close + 2 + lay + 1;
    validate(&t).map_err(|mut e| {
        match e.field {
            Some("layout") => {
                if let Some(l) = e.line {
                    let file_line = l + layout_start - 1;
                    e.error =
                        e.error
                            .replacen(&format!("line {l}:"), &format!("line {file_line}:"), 1);
                    e.line = Some(file_line);
                }
            }
            Some(f) => {
                e.line = field_lines
                    .iter()
                    .rev()
                    .find(|(g, _)| *g == f)
                    .map(|(_, n)| *n)
            }
            None => {}
        }
        e
    })?;
    Ok(t)
}

/// The file text for a template (what Save writes; editable on the desktop). `parse` reads it
/// back to the same template once validated.
pub fn to_file(t: &Template) -> String {
    let list = |v: &[String]| v.iter().map(|s| quoted(s)).collect::<Vec<_>>().join(", ");
    format!(
        "---\nid: {}\nname: {}\ndescription: {}\nvocabulary: [{}]\nsections: [{}]\n---\n\
         # Instructions\n{}\n# Layout\n{}",
        t.id,
        quoted(&t.name),
        quoted(&t.description),
        list(&t.vocabulary),
        list(&t.sections),
        t.instructions.trim(),
        t.layout
    )
}

/// `v` as written in front matter: quoted only when reading it back would change it.
fn quoted(v: &str) -> String {
    if v.trim() != v || unquote(v) != v {
        format!("\"{v}\"")
    } else {
        v.to_owned()
    }
}

fn unquote(v: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|q| v.strip_prefix(*q).and_then(|x| x.strip_suffix(*q)))
        .unwrap_or(v)
}

fn list(v: &str, n: usize) -> Result<Vec<String>, TemplateError> {
    let inner = v
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .ok_or_else(|| {
            err(
                format!("Line {n} should be a list like [a, b]"),
                Some(n),
                None,
            )
        })?;
    Ok(inner
        .split(',')
        .map(|s| unquote(s.trim()).to_owned())
        .filter(|s| !s.is_empty())
        .collect())
}

fn one_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

/// F11-FR1: the rules every template meets, wherever it came from (file, editor, built-in).
pub fn validate(t: &Template) -> Result<(), TemplateError> {
    if !crate::settings::is_template_id(&t.id) {
        let e = "The id must be 1–64 characters of a-z, 0-9 and single hyphens";
        return Err(err(e, None, Some("id")));
    }
    let n = t.name.trim().chars().count();
    if n == 0 || n > MAX_NAME || !one_line(&t.name) {
        let e = format!("The name must be 1–{MAX_NAME} characters on one line");
        return Err(err(e, None, Some("name")));
    }
    if t.description.chars().count() > MAX_DESCRIPTION || !one_line(&t.description) {
        let e = format!("The description must be one line of at most {MAX_DESCRIPTION} characters");
        return Err(err(e, None, Some("description")));
    }
    for (field, items) in [("vocabulary", &t.vocabulary), ("sections", &t.sections)] {
        let bad_item = |s: &String| {
            s.trim().is_empty()
                || s.chars().count() > MAX_LIST_ITEM
                || !one_line(s)
                || s.contains([',', '[', ']'])
        };
        if items.len() > MAX_LIST_ITEMS || items.iter().any(bad_item) {
            let e = format!(
                "The {field} list takes at most {MAX_LIST_ITEMS} items of 1–{MAX_LIST_ITEM} characters, without commas or brackets"
            );
            return Err(err(e, None, Some(field)));
        }
    }
    if t.instructions
        .lines()
        .any(|l| matches!(l.trim_end(), "# Layout" | "# Instructions"))
    {
        let e = "The instructions can't contain a # Layout or # Instructions line";
        return Err(err(e, None, Some("instructions")));
    }
    let n = t.instructions.trim().chars().count();
    if n == 0 || n > MAX_INSTRUCTIONS {
        let e = format!("The instructions must be 1–{MAX_INSTRUCTIONS} characters");
        return Err(err(e, None, Some("instructions")));
    }
    if t.layout.chars().count() > MAX_LAYOUT {
        let e = format!("The layout must be at most {MAX_LAYOUT} characters");
        return Err(err(e, None, Some("layout")));
    }
    render(&t.layout, &sample_context())
        .map(|_| ())
        .map_err(|e| err(e.message, e.line, Some("layout")))
}
