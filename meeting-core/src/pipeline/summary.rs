//! F5-FR4 / FR5 / FR8: the summarise job, run after transcription or as "Summarise again" on the
//! stored transcript. The merge is made on a fresh read of the sidecar, the transcript file is
//! written before the sidecar, and a render error writes neither (the old file stays).
use super::{JobError, JobPlan, apply, finish, joined, locked, paragraphs};
use crate::jobs::{self, Cancel};
use crate::library::{self, LibError, Library};
use crate::providers::{self, CallFail, JsonAsk};
use crate::sidecar::{self, MicKind, ProviderRun, Sidecar};
use crate::status::{self, Event, Phase};
use crate::summarise;
use crate::templates::{self, Context, CtxPara, CtxProviders, CtxSection, CtxTodo, Template};
use crate::tz;
use chrono::{DateTime, FixedOffset};
use std::sync::Mutex;

/// The meeting's template from the plan, or General with a note when it no longer exists
/// (F11-FR6).
fn template_for<'a>(plan: &'a JobPlan, id: &str) -> Option<(&'a Template, Option<String>)> {
    if let Some(t) = plan.templates.iter().find(|t| t.id == id) {
        return Some((t, None));
    }
    let general = plan
        .templates
        .iter()
        .find(|t| t.id == templates::DEFAULT_ID)?;
    let note = format!("Template {id} was not found; used {}", general.name);
    Some((general, Some(note)))
}

/// `nativeSummarise` and the tail of `transcribe`. Returns `done` or failed(summarising).
pub async fn summarise(
    lib: &Library<'_>,
    lock: &Mutex<()>,
    id: &str,
    plan: &JobPlan,
    cancel: &Cancel,
) -> Result<Phase, JobError> {
    let Some((key, model)) = &plan.summarise else {
        let e = plan.summarise_problem.clone().unwrap_or_default();
        return Err(JobError::NotStarted(LibError::Invalid(e)));
    };
    let sc = locked(lock, || {
        lib.update(id, |sc, _| {
            let p = status::next(library::phase(sc), Event::StartSummarise)?;
            library::set_phase(sc, p);
            sc.error = None;
            sc.template_id
                .get_or_insert_with(|| plan.default_template.clone());
            Ok(sc.clone())
        })
    })
    .map_err(JobError::NotStarted)?;
    let fail = |msg: String| finish(lib, lock, id, Event::SummariseFailed, Some(msg));
    let tid = sc.template_id.clone().unwrap_or_default();
    let Some((tpl, note)) = template_for(plan, &tid) else {
        return fail("No template is available".into());
    };
    let Ok(started) = DateTime::parse_from_rfc3339(&sc.started_at) else {
        return fail("The meeting's start time can't be read".into());
    };
    let ask = JsonAsk {
        system: summarise::system_prompt(&tpl.instructions, &started),
        user: summarise::user_message(&joined(&sc)),
        schema: summarise::response_schema(),
        max_tokens: summarise::MAX_OUTPUT_TOKENS,
    };
    let call = |m: String| {
        let ask = &ask;
        async move {
            let f = || providers::complete_json(&plan.http, &plan.endpoints, key, &m, ask);
            jobs::with_retry(cancel, jobs::SUMMARISE_TIMEOUT, f).await
        }
    };
    let mut ran = model.clone();
    let mut r = call(model.clone()).await;
    // F4-FR3: a model rejected for structured output is retried once with the default.
    if let (Err(e), Some(fb)) = (&r, &plan.summarise_fallback)
        && e.kind == CallFail::ModelRejected
        && fb != model
    {
        ran = fb.clone();
        r = call(fb.clone()).await;
    }
    let text = match r {
        Ok(t) => t,
        Err(e) if e.kind == CallFail::TooLarge => {
            let name = providers::provider_name(key.provider);
            return fail(format!("Transcript too large for {name}"));
        }
        Err(e) => return fail(e.detail),
    };
    let x = match summarise::parse(&text) {
        Ok(x) => x,
        Err(e) => return fail(crate::scrub::scrub(&e, &[key.expose()])),
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    let first_seen = tz::local_at(lib.tz, now_ms)
        .map(|t| tz::rfc3339(&t))
        .unwrap_or_default();
    let run = ProviderRun {
        provider: key.provider,
        model: ran,
        extra: Default::default(),
    };
    locked(lock, || {
        let mut render_error = None;
        let done = lib.update(id, |cur, dir| {
            let mut next = cur.clone(); // a fresh read: a to-do ticked meanwhile survives
            summarise::merge(&mut next, x, &first_seen, summarise::new_todo_id);
            next.template_id = Some(tpl.id.clone());
            next.providers.summarise = Some(run);
            if let Some(n) = note {
                next.note = Some(match next.note.take() {
                    Some(old) if !old.contains(&n) => format!("{old}; {n}"),
                    _ => n,
                });
            }
            let md = match templates::render(&tpl.layout, &context(&next, tpl, &started)) {
                Ok(md) => md,
                Err(e) => {
                    render_error = Some(e.message);
                    return Err(LibError::Invalid("render".into()));
                }
            };
            let name = sidecar::transcript_file_name(id);
            crate::atomic::write_atomic(&dir.join(&name), md.as_bytes())?;
            next.transcript_file = Some(name);
            let p = status::next(library::phase(&next), Event::SummariseOk)?;
            library::set_phase(&mut next, p);
            *cur = next;
            Ok(p)
        });
        match (render_error, done) {
            (Some(msg), _) => apply(lib, id, Event::SummariseFailed, Some(msg)),
            (None, Err(LibError::Io(kind))) => {
                let msg = format!("The transcript can't be saved ({kind:?})");
                apply(lib, id, Event::SummariseFailed, Some(msg))
            }
            (None, done) => done,
        }
    })
    .map_err(JobError::Storage)
}

/// The layout variables for `sc` (F11-FR4). Read-only data: a template can't reach the sidecar.
pub fn context(sc: &Sidecar, tpl: &Template, started: &DateTime<FixedOffset>) -> Context {
    let run = |r: &Option<ProviderRun>| {
        r.as_ref()
            .map(|r| format!("{} {}", r.provider.id(), r.model))
            .unwrap_or_default()
    };
    let mic = sc.mic.as_ref().map(|m| {
        let kind = match m.kind {
            MicKind::Usb => "USB",
            MicKind::Bluetooth => "Bluetooth",
        };
        format!("{kind} — {}", m.name)
    });
    Context {
        title: sc.title.clone(),
        date: started.format("%Y-%m-%d").to_string(),
        weekday: started.format("%A").to_string(),
        time: started.format("%H:%M").to_string(),
        duration: templates::duration_label(sc.duration_ms),
        mic: mic.unwrap_or_default(),
        template: tpl.name.clone(),
        providers: CtxProviders {
            transcribe: run(&sc.providers.transcribe),
            summarise: run(&sc.providers.summarise),
        },
        summary: sc.summary.clone().unwrap_or_default(),
        decisions: sc.decisions.clone(),
        todos: sc
            .todos
            .iter()
            .map(|t| CtxTodo {
                text: t.text.clone(),
                owner: t.owner.clone(),
                due: t.due.clone(),
                done: t.done,
            })
            .collect(),
        sections: sc
            .sections
            .iter()
            .map(|s| CtxSection {
                heading: s.heading.clone(),
                body: s.body.clone(),
            })
            .collect(),
        transcript: paragraphs(sc)
            .into_iter()
            .map(|(marker, text)| CtxPara { marker, text })
            .collect(),
    }
}
