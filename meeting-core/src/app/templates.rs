//! F11-FR5: the template screens' core calls. Saving writes the user copy that the desktop can
//! edit; reset deletes it. Templates are validated before anything is written.
use super::App;
use crate::api::error_json;
use crate::templates::{self, BUILTINS, Entry, Origin, Template, TemplateError};
use serde::Deserialize;
use serde_json::{Value, json};

/// A template as the editor sends it; missing fields are empty.
#[derive(Deserialize, Default)]
#[serde(default)]
struct TemplateIn {
    id: String,
    name: String,
    description: String,
    vocabulary: Vec<String>,
    sections: Vec<String>,
    instructions: String,
    layout: String,
}

impl From<TemplateIn> for Template {
    fn from(t: TemplateIn) -> Template {
        let list = |v: Vec<String>| v.into_iter().map(|s| s.trim().to_owned()).collect();
        Template {
            id: t.id.trim().to_owned(),
            name: t.name.trim().to_owned(),
            description: t.description.trim().to_owned(),
            vocabulary: list(t.vocabulary),
            sections: list(t.sections),
            instructions: t.instructions.trim().to_owned(),
            layout: t.layout,
        }
    }
}

fn template_error(e: &TemplateError) -> String {
    json!({"error": e.error, "line": e.line, "field": e.field}).to_string()
}

fn parse_in(json: &str) -> Result<Template, String> {
    serde_json::from_str::<TemplateIn>(json)
        .map(Template::from)
        .map_err(|e| {
            template_error(&TemplateError {
                error: format!("template must be a JSON object: {e}"),
                line: None,
                field: None,
            })
        })
}

/// A row or a template with where it came from.
fn entry_json(e: &Entry) -> Value {
    let mut v = json!(e.template);
    let builtin = BUILTINS.iter().any(|(id, _)| *id == e.template.id);
    v["origin"] = json!(e.origin);
    v["builtin"] = json!(builtin);
    v["customised"] = json!(e.origin == Origin::Customised);
    v["problem"] = json!(e.problem);
    v["path"] = json!(e.path);
    v
}

impl App {
    /// `nativeListTemplates`: every template, built-ins first, with its origin chip
    /// (BUILT-IN / CUSTOMISED / YOURS) and any problem with the user's file.
    pub fn list_templates_json(&self) -> String {
        let rows: Vec<Value> = self.templates.list().iter().map(entry_json).collect();
        json!(rows).to_string()
    }

    /// `nativeGetTemplate`: the effective version of `id`.
    pub fn get_template_json(&self, id: &str) -> String {
        match self.templates.get(id) {
            Some(e) => entry_json(&e).to_string(),
            None => error_json("template not found"),
        }
    }

    /// `nativeSaveTemplate`: validate, then write the user copy atomically. Replies as
    /// `nativeGetTemplate`, or `{error, line, field}` (line within the field) and writes nothing.
    pub fn save_template_json(&self, json: &str) -> String {
        let t = match parse_in(json) {
            Ok(t) => t,
            Err(e) => return e,
        };
        if let Err(e) = templates::validate(&t) {
            return template_error(&e);
        }
        if let Some(other) = self.cased_twin(&t.id) {
            return template_error(&TemplateError {
                error: format!("A file named {other} already exists"),
                line: None,
                field: Some("id"),
            });
        }
        let write = std::fs::create_dir_all(self.templates.dir()).and_then(|()| {
            let path = self.templates.path_of(&t.id);
            crate::atomic::write_atomic(&path, templates::to_file(&t).as_bytes())
        });
        if let Err(e) = write {
            return error_json(&format!("can't save the template: {:?}", e.kind()));
        }
        self.get_template_json(&t.id)
    }

    /// N4: a file in the templates folder whose name is `<id>.md` in another case. On a
    /// case-insensitive folder (shared storage) saving would write into it, and the scan never
    /// reads it as a template.
    fn cased_twin(&self, id: &str) -> Option<String> {
        let want = format!("{id}.md");
        std::fs::read_dir(self.templates.dir())
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| *n != want && n.eq_ignore_ascii_case(&want))
    }

    /// `nativeResetTemplate`: delete the user's file for `id` (a customised built-in goes back
    /// to the original; a YOURS template is deleted). `{ok, origin}`, origin `null` once gone.
    pub fn reset_template_json(&self, id: &str) -> String {
        if !crate::settings::is_template_id(id) {
            return error_json("invalid template id");
        }
        match std::fs::remove_file(self.templates.path_of(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return error_json(&format!("can't delete the template: {:?}", e.kind())),
        }
        let origin = self.templates.get(id).map(|e| e.origin);
        json!({"ok": true, "origin": origin}).to_string()
    }

    /// `nativePreviewTemplate`: the layout rendered against the sample meeting, `{markdown}`,
    /// or `{error, line, field: "layout"}`. Nothing is saved.
    pub fn preview_template_json(&self, json: &str) -> String {
        let t = match parse_in(json) {
            Ok(t) => t,
            Err(e) => return e,
        };
        if t.layout.chars().count() > templates::MAX_LAYOUT {
            let e = format!(
                "The layout must be at most {} characters",
                templates::MAX_LAYOUT
            );
            return json!({"error": e, "line": null, "field": "layout"}).to_string();
        }
        let mut ctx = templates::sample_context();
        if !t.name.is_empty() {
            ctx.template = t.name.clone();
        }
        match templates::render(&t.layout, &ctx) {
            Ok(md) => json!({"markdown": md}).to_string(),
            Err(e) => json!({"error": e.message, "line": e.line, "field": "layout"}).to_string(),
        }
    }
}
