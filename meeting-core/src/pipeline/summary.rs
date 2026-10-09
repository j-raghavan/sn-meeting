//! F5-FR4 / FR5 / FR8: the summarise job, run after transcription or as "Summarise again" on the
//! stored transcript. The merge is made on a fresh read of the sidecar, the transcript file is
//! written before the sidecar, and a render error writes neither (the old file stays). The
//! layout renders outside the write lock (S16, `transcript.rs`).
use super::notes;
use super::transcript::{self, template_of};
use super::{JobError, JobPlan, Spent, finish_spent, joined, locked, paragraphs};
use crate::jobs::{self, Cancel};
use crate::library::{self, LibError, Library};
use crate::providers::{self, CallFail, JsonAsk};
use crate::sidecar::{self, MicKind, ProviderRun, Sidecar};
use crate::spend::{Kind, UsageEntry};
use crate::status::{self, Event, Phase};
use crate::summarise;
use crate::templates::{self, Context, CtxPara, CtxProviders, CtxSection, CtxTodo, Template};
use crate::tz;
use chrono::{DateTime, FixedOffset};
use std::sync::Mutex;

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
    let (sc, dir) = locked(lock, || {
        lib.update(id, |sc, dir| {
            let p = status::next(library::phase(sc), Event::StartSummarise)?;
            library::set_phase(sc, p);
            sc.error = None;
            sc.template_id
                .get_or_insert_with(|| plan.default_template.clone());
            Ok((sc.clone(), dir.to_path_buf()))
        })
    })
    .map_err(JobError::NotStarted)?;
    // F19-FR6/FR7: the note regions to recognise in the same call, when there are any: all
    // of them the first time, then only those written on since.
    let prepared = plan.notes.get(&dir, &sc).and_then(|n| {
        let stored = crate::notes_text::read(&crate::notes_text::path(&dir, id));
        let before = sc.extra.contains_key("notesRecognisedAt");
        notes::prepare(n, stored.unwrap_or_default(), before)
    });
    let notes_in = prepared.as_ref().map(|p| &p.send);
    let spent = Spent::new(lib.tz, &plan.rates);
    let fail = |msg: String| finish_spent(lib, lock, id, Event::SummariseFailed, Some(msg), &spent);
    let tid = sc.template_id.clone().unwrap_or_default();
    let Some((tpl, note)) = template_of(&plan.templates, &tid) else {
        return fail("No template is available".into());
    };
    let Ok(started) = DateTime::parse_from_rfc3339(&sc.started_at) else {
        return fail("The meeting's start time can't be read".into());
    };
    // F18-FR12: a linked meeting's event goes before the transcript, as data.
    let event = plan
        .event_block
        .then(|| crate::calendar::link::event_of(&sc))
        .flatten()
        .map(|e| summarise::event_block(&e));
    let user = match &event {
        Some(b) => summarise::user_message_with_event(&joined(&sc), b),
        None => summarise::user_message(&joined(&sc)),
    };
    let with_event = |system: String| match &event {
        Some(_) => format!("{system}\n{}", summarise::EVENT_RULE),
        None => system,
    };
    let ask = match &notes_in {
        None => JsonAsk {
            system: with_event(summarise::system_prompt(&tpl.instructions, &started)),
            user,
            schema: summarise::response_schema(),
            max_tokens: summarise::MAX_OUTPUT_TOKENS,
            images: Vec::new(),
        },
        Some(n) => JsonAsk {
            system: with_event(summarise::system_prompt_with_notes(
                &tpl.instructions,
                &started,
            )),
            user,
            schema: summarise::response_schema_with_notes(),
            max_tokens: summarise::MAX_OUTPUT_TOKENS,
            images: notes::images(n, 0),
        },
    };
    let call = |m: String| {
        let (ask, spent) = (&ask, &spent);
        async move {
            let f = || {
                let fut = providers::complete_json(&plan.http, &plan.endpoints, key, &m, ask);
                async move {
                    let r = fut.await;
                    spent.note(Kind::Summarise, None, &r);
                    r
                }
            };
            jobs::with_retry(cancel, plan.budgets.summarise, f).await
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
        Ok(t) => t.text,
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
    let recognised = match notes_in {
        Some(n) => Some(notes::recognise(plan, key, &ran, n, &text, &spent, cancel).await),
        None => None,
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
    // The merged sidecar `cur` becomes: a fresh read, so a to-do ticked meanwhile survives.
    let merged = |cur: &Sidecar| {
        let mut next = cur.clone();
        summarise::merge(&mut next, x.clone(), &first_seen, summarise::new_todo_id);
        // Record the fallback only if the meeting still names the template the job began
        // with: a template chosen meanwhile is the one the next run uses (F11-FR6).
        if note.is_some() && next.template_id.as_deref() == Some(tid.as_str()) {
            next.template_id = Some(tpl.id.clone());
        }
        next.providers.summarise = Some(run.clone());
        if let Some(n) = &note {
            next.note = Some(match next.note.take() {
                Some(old) if !old.contains(n.as_str()) => format!("{old}; {n}"),
                _ => n.clone(),
            });
        }
        next
    };
    // S16: render outside the write lock; under it, re-read and re-merge, and write the file
    // only if what the layout sees is unchanged (else render again; the last try is locked).
    let name = sidecar::transcript_file_name(id);
    let mut attempt = 0;
    loop {
        attempt += 1;
        let snap = match locked(lock, || lib.load_for_write(id)) {
            Ok((_, sc)) => merged(&sc),
            Err(e) => return Err(JobError::Storage(e)),
        };
        // A render error writes neither the file nor the merge.
        let ctx = match transcript::context_of(&snap, tpl) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let md = match transcript::render(tpl, &ctx) {
            Ok(md) => md,
            Err(e) => return fail(e),
        };
        let last = attempt == transcript::ATTEMPTS;
        let r = locked(lock, || -> Result<Option<Phase>, LibError> {
            let (dir, cur) = lib.load_for_write(id)?;
            let mut next = merged(&cur);
            let now = transcript::context_of(&next, tpl).map_err(LibError::Invalid)?;
            let md = if now == ctx {
                md
            } else if last {
                transcript::render(tpl, &now).map_err(LibError::Invalid)?
            } else {
                return Ok(None);
            };
            crate::atomic::write_atomic(&dir.join(&name), md.as_bytes())?;
            next.transcript_file = Some(name.clone());
            // F19-FR6: the recognised text beside the ink (never the ink itself, INV12).
            if let (Some(r), Some(p)) = (&recognised, &prepared) {
                let all = notes::stored_after(p, r.regions.clone());
                crate::notes_text::write(&dir, id, &all)?;
                if r.complete {
                    next.extra
                        .insert("notesRecognisedAt".into(), first_seen.clone().into());
                }
            }
            let p = status::next(library::phase(&next), Event::SummariseOk)?;
            library::set_phase(&mut next, p);
            // F16-FR2: the summary's usage goes in the write that stores it.
            let recs = spent.take();
            next.usage
                .extend(recs.iter().cloned().map(UsageEntry::from));
            if let Err(e) = sidecar::write(&dir, &next) {
                spent.put_back(recs);
                return Err(e.into());
            }
            Ok(Some(p))
        });
        match r {
            Ok(Some(p)) => return Ok(p),
            Ok(None) => continue,
            Err(LibError::Io(kind)) => {
                return fail(format!("The transcript can't be saved ({kind:?})"));
            }
            Err(LibError::Invalid(msg)) => return fail(msg),
            Err(e) => return Err(JobError::Storage(e)),
        }
    }
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
