//! F15-FR5 / FR7: the near-live lane's entry points. `nativeStartLive` fixes who transcribes
//! the chunks when recording begins; `nativeLiveDrain` sends the pending ones, one at a time and
//! in order, until none is left, one fails (the reply says when to try again) or the lane is
//! paused. The shell's lane thread calls it after each chunk and on its own retry schedule; it
//! runs beside the job lane and the short-call thread (M2/M3 ruling), never on either.
use super::App;
use crate::api::error_json;
use crate::jobs::{self, Cancel};
use crate::keyfile::{self, Provider, ProviderKey};
use crate::pipeline::live::{self, ChunkCall, Outcome};
use crate::providers;
use crate::settings::{self, ModelSource, Role, Settings};
use crate::sidecar::{ChunkState, Live, Sidecar};
use crate::status::Status;
use serde_json::{Value, json};
use std::sync::Arc;

/// `live.paused` when the user recorded without near-live transcription (F15-FR12).
pub const DECLINED: &str = "Not sent while recording. Tap Transcribe when you stop.";

/// What the lane does now, from the sidecar (`None`: send the chunk).
fn lane_state(sc: &Sidecar) -> Option<Value> {
    if sc.status != Status::Recording {
        return Some(json!({"state": "stopped"}));
    }
    let Some(l) = &sc.live else {
        return Some(json!({"state": "off"}));
    };
    if let Some(r) = &l.paused {
        return Some(json!({"state": "paused", "reason": r}));
    }
    None
}

/// The pending chunks of `sc`.
fn waiting(sc: &Sidecar) -> usize {
    sc.chunks
        .iter()
        .filter(|c| c.state == ChunkState::Pending)
        .count()
}

impl App {
    /// The lock whoever transcribes `id`'s chunks holds: the lane, or after Stop the job
    /// (F15-FR9), so a chunk is never sent twice at once (INV11).
    pub(crate) fn lane(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut m = self
            .lanes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.entry(id.to_owned()).or_default().clone()
    }

    /// The transcription model for `key`'s provider as Settings choose it now: an override
    /// (when that provider is the role's), the key file's `transcribe_model`, or the default.
    fn live_model(&self, s: &Settings, key: &ProviderKey) -> (String, ModelSource) {
        let usable = |p| settings::key_status(&self.core.key_dir, p).usable();
        let role_provider = settings::effective_provider(s, Role::Transcribe, usable);
        let over = (role_provider == key.provider)
            .then(|| s.transcribe.model.clone())
            .flatten();
        let from_key = Some((key.model.as_str(), key.transcribe_model.as_deref()));
        settings::effective_model(Role::Transcribe, key.provider, over.as_deref(), from_key)
            .unwrap_or((String::new(), ModelSource::Default))
    }

    /// `nativeStartLive(id, send)` (F15-FR5 / FR12): the near-live recorder began `id`. The
    /// transcribe provider (and the model, when one is chosen) is fixed now; a default model is
    /// picked from the account's listing when the first chunk goes. With `send` false (the
    /// notice declined) the lane stays paused and the chunks wait for Transcribe.
    pub fn start_live_json(&self, id: &str, send: bool) -> String {
        let (s, _) = self.settings();
        let usable = |p| settings::key_status(&self.core.key_dir, p).usable();
        let provider = settings::effective_provider(&s, Role::Transcribe, usable);
        let model = keyfile::load(&self.core.key_dir, provider)
            .ok()
            .map(|k| self.live_model(&s, &k))
            .filter(|(_, src)| *src != ModelSource::Default)
            .map(|(m, _)| m);
        let _w = self.write_lock();
        let r = self.library().update(id, |sc, _| {
            if sc.status != Status::Recording {
                return Err(crate::library::LibError::Invalid(
                    "meeting is not recording".into(),
                ));
            }
            sc.live = Some(Live {
                provider,
                model: model.clone(),
                paused: (!send).then(|| DECLINED.to_owned()),
                extra: Default::default(),
            });
            Ok(json!({"provider": provider, "model": model, "send": send}))
        });
        match r {
            Ok(v) => v.to_string(),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Pause the lane of `id` with `reason` (shown once on the meeting page).
    fn pause(&self, id: &str, reason: &str) {
        let _w = self.write_lock();
        let _ = self.library().update(id, |sc, _| {
            if let Some(l) = &mut sc.live {
                l.paused = Some(reason.to_owned());
            }
            Ok(())
        });
    }

    /// The model the lane sends `id`'s chunks with: the one fixed at Record, or (the default)
    /// the account's first transcription model, stored so every chunk uses the same.
    fn lane_model(
        &self,
        id: &str,
        live: &Live,
        key: &ProviderKey,
        http: &providers::Http,
    ) -> String {
        if let Some(m) = &live.model {
            return m.clone();
        }
        let (s, _) = self.settings();
        let (mut m, src) = self.live_model(&s, key);
        if src == ModelSource::Default
            && let Some(auto) = self
                .listed_models(key, http)
                .and_then(|ids| settings::pick_transcribe(key.provider, &ids))
        {
            m = auto;
        }
        let _w = self.write_lock();
        let _ = self.library().update(id, |sc, _| {
            if let Some(l) = &mut sc.live {
                l.model.get_or_insert_with(|| m.clone());
            }
            Ok(())
        });
        m
    }

    /// `nativeLiveDrain(id)` (F15-FR5): blocking. Sends `id`'s pending chunks in order while it
    /// is recording. Replies `{state, done, waiting, retryInMs?, reason?}`, `state` being
    /// `idle` (nothing pending), `retry` (try again after `retryInMs`), `paused`, `blocked` (the
    /// next chunk's file is gone: re-cut at Stop), `stopped` (no longer recording), `off` (not a
    /// near-live meeting) or `busy` (another caller is sending them).
    pub fn live_drain_json(&self, id: &str) -> String {
        if !crate::sidecar::is_valid_id(id) {
            return error_json("invalid meeting id");
        }
        let lane = self.lane(id);
        let Ok(_held) = lane.try_lock() else {
            return json!({"state": "busy"}).to_string();
        };
        let http = match providers::http_client(jobs::CONNECT_TIMEOUT, self.core.https_only) {
            Ok(h) => h,
            Err(e) => return error_json(&e),
        };
        let root = self.chunk_root();
        let cancel = Cancel::default();
        let mut done = 0usize;
        loop {
            let sc = match self.library().load_for_write(id) {
                Ok((_, sc)) => sc,
                Err(e) => return error_json(&e.to_string()),
            };
            let reply = |mut v: Value| {
                v["done"] = json!(done);
                v["waiting"] = json!(waiting(&sc));
                v.to_string()
            };
            if let Some(v) = lane_state(&sc) {
                return reply(v);
            }
            let Some(next) = sc.chunks.iter().find(|c| c.state == ChunkState::Pending) else {
                return reply(json!({"state": "idle"}));
            };
            let live = sc.live.clone().expect("lane_state checked it");
            let key = match keyfile::load(&self.core.key_dir, live.provider) {
                Ok(k) => k,
                Err(e) => {
                    let r = live::paused_reason(&key_detail(live.provider, e));
                    self.pause(id, &r);
                    return reply(json!({"state": "paused", "reason": r}));
                }
            };
            let model = self.lane_model(id, &live, &key, &http);
            let tid = sc
                .template_id
                .clone()
                .unwrap_or_else(|| self.effective_default(&self.settings().0));
            let rates = self.rates();
            let call = ChunkCall {
                http: &http,
                endpoints: &self.core.endpoints,
                key: &key,
                model: &model,
                budgets: self.budgets,
                rates: &rates,
                vocabulary: crate::pipeline::vocabulary_of(&self.templates.effective(), &tid),
            };
            let index = next.index;
            let lib = self.library();
            let fut = live::attempt(&lib, &self.write, id, &root, &call, index, &cancel);
            let outcome = match crate::runtime::block_on(fut) {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => return error_json(&e.to_string()),
                Err(e) => return error_json(&format!("runtime: {e}")),
            };
            match outcome {
                Outcome::Stored => {
                    done += 1;
                    let _ = lib.prune_chunks(id, &root);
                }
                Outcome::Again => {}
                Outcome::Retry(wait) => return reply(json!({"state": "retry", "retryInMs": wait})),
                Outcome::Pause(r) => {
                    self.pause(id, &r);
                    return reply(json!({"state": "paused", "reason": r}));
                }
                Outcome::NoFile => return reply(json!({"state": "blocked"})),
                Outcome::Cancelled => return reply(json!({"state": "stopped"})),
            }
        }
    }
}

/// F15-FR6 / FR7: the live transcript's status: `off` (not near-live), `paused` (with the
/// reason), `offline` (the next chunk failed and waits for the back-off), `transcribing`
/// (chunks waiting) or `current`.
fn live_status(sc: &Sidecar) -> (&'static str, Option<String>) {
    if sc.live.is_none() && sc.chunks.is_empty() {
        return ("off", None);
    }
    if let Some(r) = sc.live.as_ref().and_then(|l| l.paused.clone())
        && sc.status == Status::Recording
    {
        return ("paused", Some(r));
    }
    match sc.chunks.iter().find(|c| c.state == ChunkState::Pending) {
        None => ("current", None),
        Some(c) if c.attempts > 0 && c.last_error.as_deref() != Some(live::EMPTY_ONCE) => {
            ("offline", c.last_error.clone())
        }
        Some(_) => ("transcribing", None),
    }
}

impl App {
    /// `nativeLiveTranscript(id, afterLine)` (F15-FR6, F17-FR6): the transcript's whole lines
    /// so far, numbered from 1, each with its estimated recording-clock time, after line
    /// `afterLine` (0: all). `{lines: [{n, atMs, text}], waiting, state, reason?, spentUsd,
    /// spent}`: the strip appends lines and shows "Offline · 4 waiting" (F15-FR7); `spent` is
    /// "≈ $0.08" (F16-FR5).
    pub fn live_transcript_json(&self, id: &str, after_line: i32) -> String {
        let sc = match self.library().get(id) {
            Ok(v) => v.meeting,
            Err(e) => return error_json(&e.to_string()),
        };
        let mut lines = Vec::new();
        for p in crate::chunk::paragraphs(&sc) {
            let Some(c) = sc.chunks.iter().find(|c| c.index == p.index) else {
                continue;
            };
            for (at, text) in crate::chunk::lines(&p, c) {
                lines.push(json!({"n": lines.len() + 1, "atMs": at, "text": text}));
            }
        }
        let after = usize::try_from(after_line).unwrap_or(0);
        let (state, reason) = live_status(&sc);
        let total = crate::spend::meeting_total(&sc);
        let mut v = json!({
            "lines": lines.into_iter().skip(after).collect::<Vec<_>>(),
            "waiting": waiting(&sc),
            "state": state,
            "spentUsd": total.usd,
            "spent": total.label,
        });
        if let Some(r) = reason {
            v["reason"] = json!(r);
        }
        v.to_string()
    }
}

/// The F4-FR5 wording of a key problem.
fn key_detail(p: Provider, e: keyfile::KeyError) -> String {
    settings::key_status_of(p, &Err(e))
        .detail()
        .map(str::to_owned)
        .unwrap_or_default()
}
