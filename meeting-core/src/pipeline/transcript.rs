//! F5-FR4 / F5-FR11: the transcript file `<id>.transcript.md`, rendered from the sidecar with the
//! meeting's template. It is written once transcription ends (with an empty summary), after a
//! summary, and again after every edit it shows (a tick, a rename, a template change).
//!
//! A render runs outside the core write lock, so a slow layout never holds up the recorder
//! (`add_segment`) or another edit: the sidecar is read, the layout rendered, then under the lock
//! the sidecar is read again and the file written only if what the layout sees is unchanged.
//! Otherwise it is rendered again; the last attempt renders under the lock, so it always ends.
use super::locked;
use super::summary::context;
use crate::library::{LibError, Library};
use crate::sidecar::{self, Sidecar};
use crate::templates::{self, Context, Template};
use chrono::DateTime;
use std::sync::Mutex;

/// Renders tried outside the lock before the last one, under it.
pub(crate) const ATTEMPTS: usize = 3;

/// The meeting's template from `templates`, or General with a note when it no longer exists
/// (F11-FR6).
pub(crate) fn template_of<'a>(
    templates: &'a [Template],
    id: &str,
) -> Option<(&'a Template, Option<String>)> {
    if let Some(t) = templates.iter().find(|t| t.id == id) {
        return Some((t, None));
    }
    let general = templates.iter().find(|t| t.id == templates::DEFAULT_ID)?;
    let note = format!("Template {id} was not found; used {}", general.name);
    Some((general, Some(note)))
}

/// What the layout sees for `sc` with `tpl`, or why it can't be built.
pub(crate) fn context_of(sc: &Sidecar, tpl: &Template) -> Result<Context, String> {
    let started = DateTime::parse_from_rfc3339(&sc.started_at)
        .map_err(|_| "The meeting's start time can't be read".to_owned())?;
    Ok(context(sc, tpl, &started))
}

/// `tpl`'s layout rendered with `ctx`; the error is the user-facing "Template error …".
pub(crate) fn render(tpl: &Template, ctx: &Context) -> Result<String, String> {
    templates::render(&tpl.layout, ctx).map_err(|e| e.message)
}

/// The template a meeting's file uses now: its own, or General.
fn template_for<'a>(sc: &Sidecar, templates: &'a [Template]) -> Result<&'a Template, String> {
    let id = sc.template_id.as_deref().unwrap_or(templates::DEFAULT_ID);
    template_of(templates, id)
        .map(|(t, _)| t)
        .ok_or_else(|| "No template is available".to_owned())
}

/// Render `id`'s transcript file from its sidecar and write it atomically. Only a meeting that
/// already has one is re-rendered, unless `create` (transcription just ended). Errors are
/// user-facing; the sidecar is never changed except to record the file's name.
pub fn rewrite(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    templates: &[Template],
    create: bool,
) -> Result<(), String> {
    let name = sidecar::transcript_file_name(id);
    let mut attempt = 0;
    loop {
        attempt += 1;
        let (_, sc) = locked(lock, || lib.load_for_write(id)).map_err(|e| e.to_string())?;
        if sc.transcript_file.is_none() && !create {
            return Ok(());
        }
        let tpl = template_for(&sc, templates)?;
        let ctx = context_of(&sc, tpl)?;
        let md = render(tpl, &ctx)?;
        let wrote = locked(lock, || -> Result<Result<bool, String>, LibError> {
            let (dir, mut cur) = lib.load_for_write(id)?;
            let tpl = match template_for(&cur, templates) {
                Ok(t) => t,
                Err(e) => return Ok(Err(e)),
            };
            let now = match context_of(&cur, tpl) {
                Ok(c) => c,
                Err(e) => return Ok(Err(e)),
            };
            let md = if now == ctx {
                md
            } else if attempt == ATTEMPTS {
                match render(tpl, &now) {
                    Ok(m) => m,
                    Err(e) => return Ok(Err(e)),
                }
            } else {
                return Ok(Ok(false)); // changed meanwhile: render again outside the lock
            };
            crate::atomic::write_atomic(&dir.join(&name), md.as_bytes())?;
            if cur.transcript_file.as_deref() != Some(name.as_str()) {
                cur.transcript_file = Some(name.clone());
                sidecar::write(&dir, &cur)?;
            }
            Ok(Ok(true))
        });
        match wrote {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => continue,
            Ok(Err(e)) => return Err(e),
            Err(LibError::Io(kind)) => {
                return Err(format!("The transcript can't be saved ({kind:?})"));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}
