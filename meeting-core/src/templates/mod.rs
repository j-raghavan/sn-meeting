//! F11: templates. A template is a Markdown file: `---` front matter, then `# Instructions`
//! (the summarise system prompt) and `# Layout` (a minijinja layout for the transcript file).
//! It changes only the prompt text and the layout (INV9).
use serde::Serialize;

mod render;
pub use render::{
    Context, CtxPara, CtxProviders, CtxSection, CtxTodo, RenderError, duration_label, render,
};

pub const DEFAULT_ID: &str = "general";

/// The built-ins, compiled in (F11-FR2).
pub const BUILTINS: [(&str, &str); 1] = [("general", include_str!("../../templates/general.md"))];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub vocabulary: Vec<String>,
    pub sections: Vec<String>,
    pub instructions: String,
    pub layout: String,
}

/// Split a template file into its parts. `None` when a part is missing.
pub fn parse(text: &str) -> Option<Template> {
    let text = text.replace("\r\n", "\n");
    let body = text.strip_prefix("---\n")?;
    let (front, rest) = body.split_once("\n---\n")?;
    let mut t = Template {
        id: String::new(),
        name: String::new(),
        description: String::new(),
        vocabulary: Vec::new(),
        sections: Vec::new(),
        instructions: String::new(),
        layout: String::new(),
    };
    for line in front.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "id" => t.id = v.into(),
            "name" => t.name = v.into(),
            "description" => t.description = v.into(),
            _ => {}
        }
    }
    let rest = rest.strip_prefix("# Instructions\n")?;
    let (ins, layout) = rest.split_once("\n# Layout\n")?;
    t.instructions = ins.trim().into();
    t.layout = layout.into();
    Some(t)
}

/// The built-in templates, parsed.
pub fn builtins() -> Vec<Template> {
    BUILTINS.iter().filter_map(|(_, src)| parse(src)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_general_builtin_parses() {
        let b = builtins();
        assert_eq!(b.len(), BUILTINS.len());
        assert_eq!(
            (b[0].id.as_str(), b[0].name.as_str()),
            ("general", "General meeting")
        );
        assert!(
            b[0].instructions
                .starts_with("You are summarising a meeting")
        );
        assert!(
            b[0].layout
                .starts_with("# {{ title or \"Untitled meeting\" }}\n")
        );
        for bad in [
            "",
            "---\nid: x\n",
            "---\nid: x\n---\nno headings",
            "---\n---\n# Instructions\nx",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}
