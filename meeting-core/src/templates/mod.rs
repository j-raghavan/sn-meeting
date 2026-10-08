//! F11: templates. A template is a Markdown file: `---` front matter, then `# Instructions`
//! (the summarise system prompt) and `# Layout` (a minijinja layout for the transcript file).
//! It changes only the prompt text and the layout (INV9).
use serde::Serialize;

mod file;
mod render;
pub use file::{
    MAX_DESCRIPTION, MAX_FILE_BYTES, MAX_INSTRUCTIONS, MAX_LAYOUT, MAX_LIST_ITEM, MAX_LIST_ITEMS,
    MAX_NAME, TemplateError, parse, validate,
};
pub use render::{
    Context, CtxPara, CtxProviders, CtxSection, CtxTodo, FUEL, MAX_OUTPUT, RenderError,
    duration_label, render, sample_context,
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

impl Template {
    pub(crate) fn empty() -> Template {
        Template {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            vocabulary: Vec::new(),
            sections: Vec::new(),
            instructions: String::new(),
            layout: String::new(),
        }
    }
}

/// The built-in templates, parsed.
pub fn builtins() -> Vec<Template> {
    BUILTINS
        .iter()
        .filter_map(|(_, src)| parse(src).ok())
        .collect()
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
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
