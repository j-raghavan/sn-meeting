//! Process state behind the JNI API: `Core` (M1, immutable), the settings in use, the device
//! time zone and what is live (recording, jobs).
use crate::api::{self, Core, error_json};
use crate::keyfile::Provider;
use crate::library::{Library, Liveness};
use crate::settings::{self, Ctx, KeyStatus, Origin, Role, Settings};
use crate::sidecar::{Audio, Mic};
use crate::tz::{DeviceTz, LocalTz};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

mod run;
mod spend;
mod templates;
pub use run::Job;

#[derive(Debug)]
pub struct App {
    pub core: Core,
    /// `/storage/emulated/0` on the device; a tempdir in tests.
    pub storage_root: PathBuf,
    pub tz: Arc<dyn LocalTz>,
    state: Mutex<State>,
    /// The one sidecar write lock (F3-FR2): held across load → change → write of every
    /// mutation, so two threads never lose or tear each other's write. Taken before `state`.
    write: Mutex<()>,
    /// Transcribe/summarise jobs in flight, with their cancel flags (F5-FR7).
    pub jobs: crate::jobs::Registry,
    /// Whole-call budgets for jobs (F5-FR7); only tests change them.
    pub budgets: crate::jobs::Budgets,
    /// The user's templates folder under the storage root, and the parsed set (F11-FR3).
    pub templates: crate::templates::Store,
    /// D2: each provider's model listing, once read, for the life of the process.
    models: Mutex<HashMap<Provider, Vec<String>>>,
}

#[derive(Debug)]
struct State {
    settings: Settings,
    origin: Origin,
    live: Liveness,
}

impl App {
    /// Load `settings.json` from `core.app_data_dir`. Never fails (ADR D10).
    pub fn new(core: Core, storage_root: PathBuf) -> App {
        App::with_tz(core, storage_root, Arc::new(DeviceTz))
    }

    /// `new` with an explicit time zone (tests use a fixed offset).
    pub fn with_tz(core: Core, storage_root: PathBuf, tz: Arc<dyn LocalTz>) -> App {
        let (settings, origin) = settings::load(&settings::path_in(&core.app_data_dir));
        let templates = crate::templates::Store::under(&storage_root);
        App {
            core,
            storage_root,
            tz,
            state: Mutex::new(State {
                settings,
                origin,
                live: Liveness::default(),
            }),
            write: Mutex::new(()),
            jobs: Default::default(),
            budgets: Default::default(),
            templates,
            models: Mutex::default(),
        }
    }

    /// The on-device app: `Core::device` and the shared-storage root.
    pub fn device(app_data_dir: &str) -> App {
        App::new(Core::device(app_data_dir), settings::STORAGE_ROOT.into())
    }

    /// Poison-tolerant: a panic caught by `api::guarded` must not wedge every later call.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold this across every sidecar read-modify-write. Poison-tolerant, as `state`.
    pub fn write_lock(&self) -> MutexGuard<'_, ()> {
        self.write.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A file that couldn't be read at launch is tried again (N3).
    pub fn settings(&self) -> (Settings, Origin) {
        let mut st = self.state();
        if st.origin == Origin::Unreadable {
            (st.settings, st.origin) = settings::load(&settings::path_in(&self.core.app_data_dir));
        }
        (st.settings.clone(), st.origin)
    }

    /// D2: `p`'s model listing, if it has been read in this process.
    pub fn cached_models(&self, p: Provider) -> Option<Vec<String>> {
        let m = self.models.lock().unwrap_or_else(PoisonError::into_inner);
        m.get(&p).cloned()
    }

    fn store_models(&self, p: Provider, ids: Vec<String>) {
        let mut m = self.models.lock().unwrap_or_else(PoisonError::into_inner);
        m.insert(p, ids);
    }

    /// D2: the key's provider's model listing, read once per process (the call Test connection
    /// makes). `None` when it can't be read; it is tried again next time. Blocking.
    pub(crate) fn listed_models(
        &self,
        key: &crate::keyfile::ProviderKey,
        http: &crate::providers::Http,
    ) -> Option<Vec<String>> {
        if let Some(ids) = self.cached_models(key.provider) {
            return Some(ids);
        }
        let ep = &self.core.endpoints;
        let fut = crate::providers::list_models(http, ep, key, self.core.timeout);
        let ids = crate::runtime::block_on(fut).ok().flatten()?;
        self.store_models(key.provider, ids.clone());
        Some(ids)
    }

    /// What is live right now: the recording, and every meeting with a job in flight.
    pub fn liveness(&self) -> Liveness {
        Self::live_of(&self.state(), &self.jobs)
    }

    fn live_of(st: &State, jobs: &crate::jobs::Registry) -> Liveness {
        let mut live = st.live.clone();
        live.jobs.extend(jobs.running());
        live
    }

    /// The library over the current settings' folders, current first.
    pub fn library(&self) -> Library<'_> {
        self.library_of(&self.state().settings)
    }

    fn library_of(&self, s: &Settings) -> Library<'_> {
        Library {
            folders: s.folders(&self.storage_root),
            tz: self.tz.as_ref(),
        }
    }

    /// `nativeBeginMeeting`. One recording at a time: the meeting stays live until finished.
    pub fn begin_json(&self, started_at_ms: i64, mic_json: &str, audio_json: &str) -> String {
        let mic: Mic = match serde_json::from_str(mic_json) {
            Ok(m) => m,
            Err(e) => return error_json(&format!("micJson: {e}")),
        };
        let audio: Audio = match serde_json::from_str(audio_json) {
            Ok(a) => a,
            Err(e) => return error_json(&format!("audioJson: {e}")),
        };
        let _w = self.write_lock();
        let mut st = self.state();
        if let Some(active) = &st.live.recording {
            return error_json(&format!("already recording {active}"));
        }
        let debug_ms = st.settings.debug.segment_ms;
        match self
            .library_of(&st.settings)
            .begin(started_at_ms, mic, audio, debug_ms)
        {
            Ok(b) => {
                st.live.recording = Some(b.id.clone());
                json!(b).to_string()
            }
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// `nativeAddSegment`.
    pub fn add_segment_json(&self, id: &str, seg_path: &str, start_offset_ms: i64) -> String {
        let _w = self.write_lock();
        let lib = self.library();
        reply(
            lib.add_segment(id, seg_path, start_offset_ms)
                .map(|a| json!(a)),
        )
    }

    /// `nativeCloseSegment`; `index` is 0-based.
    pub fn close_segment_json(&self, id: &str, index: i32, duration_ms: i64) -> String {
        let Ok(index) = usize::try_from(index) else {
            return error_json("segIndex must be >= 0");
        };
        let _w = self.write_lock();
        reply(
            self.library()
                .close_segment(id, index, duration_ms)
                .map(|()| ok()),
        )
    }

    /// `nativeFinishMeeting`; an empty `note` means none. The recording is over either way,
    /// so the meeting stops being live even if the sidecar can't be updated.
    pub fn finish_json(&self, id: &str, duration_ms: i64, note: &str) -> String {
        if duration_ms < 0 {
            // A rejected call changes nothing: the recording stays live (F3-FR2).
            return error_json("durationMs must be >= 0");
        }
        let _w = self.write_lock();
        let mut st = self.state();
        if st.live.recording.as_deref() == Some(id) {
            st.live.recording = None;
        }
        let lib = self.library_of(&st.settings);
        reply(lib.finish(id, duration_ms, Some(note)).map(|()| ok()))
    }

    /// `nativeListRecent(limit)`: the newest meetings across every save folder.
    pub fn list_recent_json(&self, limit: i32) -> String {
        match usize::try_from(limit) {
            Ok(n) => json!(self.library().list_recent(n)).to_string(),
            Err(_) => error_json("limit must be >= 0"),
        }
    }

    /// `nativeGetMeeting`.
    pub fn get_meeting_json(&self, id: &str) -> String {
        reply(self.library().get(id).map(|v| json!(v)))
    }

    /// `nativeOpenTodos`: open to-dos from every meeting (F6-FR1).
    pub fn open_todos_json(&self) -> String {
        json!(self.library().open_todos()).to_string()
    }

    /// S4: after an edit the transcript file shows, render it again (outside the write lock).
    /// Best effort: the edit itself is already saved.
    fn edited(&self, id: &str, r: Result<(), crate::library::LibError>) -> String {
        if r.is_ok() {
            let templates = self.templates.effective();
            let _ = crate::pipeline::rewrite_transcript(
                &self.library(),
                &self.write,
                id,
                &templates,
                false,
            );
        }
        reply(r.map(|()| ok()))
    }

    /// `nativeSetTodoDone` (F6-FR2).
    pub fn set_todo_done_json(&self, id: &str, todo_id: &str, done: bool) -> String {
        let r = {
            let _w = self.write_lock();
            self.library().set_todo_done(id, todo_id, done)
        };
        self.edited(id, r)
    }

    /// `nativeRenameMeeting` (F7-FR5); an empty title clears it.
    pub fn rename_json(&self, id: &str, title: &str) -> String {
        let r = {
            let _w = self.write_lock();
            self.library().rename(id, title)
        };
        self.edited(id, r)
    }

    /// `nativeSetMeetingTemplate` (F7-FR2).
    pub fn set_template_json(&self, id: &str, template_id: &str) -> String {
        // F11-FR6: only a template the next job could use (an id is checked by the library).
        if settings::is_template_id(template_id) && !self.template_usable(template_id) {
            return error_json("template not found");
        }
        let r = {
            let _w = self.write_lock();
            self.library().set_template(id, template_id)
        };
        self.edited(id, r)
    }

    /// `nativeDeleteMeeting` (F7-FR4): `{ok, removed: [file names]}`.
    pub fn delete_json(&self, id: &str) -> String {
        let _w = self.write_lock();
        let st = self.state();
        let lib = self.library_of(&st.settings);
        reply(
            lib.delete(id, &Self::live_of(&st, &self.jobs))
                .map(|removed| json!({"ok": true, "removed": removed})),
        )
    }

    /// `nativeRecover(serviceAlive)` (F3-FR5). A dead service means nothing is recording.
    pub fn recover_json(&self, service_alive: bool) -> String {
        let _w = self.write_lock();
        let mut st = self.state();
        if !service_alive {
            st.live.recording = None;
        }
        let lib = self.library_of(&st.settings);
        json!(lib.recover(&Self::live_of(&st, &self.jobs))).to_string()
    }

    /// Whether `id` is in the effective template set and usable by a job.
    fn template_usable(&self, id: &str) -> bool {
        self.templates.get(id).is_some_and(|e| e.usable())
    }

    /// The default template a job stamps: Settings' choice, or General when that template is
    /// gone (a YOURS file deleted on the desktop).
    pub fn effective_default(&self, s: &Settings) -> String {
        if self.template_usable(&s.default_template) {
            s.default_template.clone()
        } else {
            crate::templates::DEFAULT_ID.to_owned()
        }
    }

    /// Every provider's key status, read fresh (the user may add a file at any time).
    fn key_statuses(&self) -> Vec<(Provider, KeyStatus)> {
        Provider::ALL
            .iter()
            .map(|p| (*p, settings::key_status(&self.core.key_dir, *p)))
            .collect()
    }

    /// `nativeGetSettings`: the settings, where they came from, what will actually run for each
    /// role, and each provider's key status (F10-FR4). Never contains a key (INV2).
    pub fn get_settings_json(&self) -> String {
        let (s, origin) = self.settings();
        let keys = self.key_statuses();
        let status = |p: Provider| &keys.iter().find(|(q, _)| *q == p).expect("all providers").1;
        let effective = |role: Role| {
            let p = settings::effective_provider(&s, role, |p| status(p).usable());
            let from_key = match status(p) {
                KeyStatus::Found {
                    model,
                    transcribe_model,
                    ..
                } => Some((model.as_str(), transcribe_model.as_deref())),
                _ => None,
            };
            let mut m = settings::effective_model(role, p, s.role(role).model.as_deref(), from_key);
            // D2: once the account's models are known, the one a job would pick (the code
            // default when it lists none); before, the first candidate, checked on first use.
            if let Some((first, settings::ModelSource::Default)) = &m
                && role == Role::Transcribe
            {
                m = Some(match self.cached_models(p) {
                    None => (first.clone(), settings::ModelSource::AutoUnchecked),
                    Some(ids) => match settings::pick_transcribe(p, &ids) {
                        Some(auto) => (auto, settings::ModelSource::Auto),
                        None => (first.clone(), settings::ModelSource::Default),
                    },
                });
            }
            let (model, source) = m.map_or((None, None), |(m, src)| (Some(m), Some(src)));
            json!({"provider": p, "model": model, "modelSource": source})
        };
        let keys_json: Map<String, Value> = keys
            .iter()
            .map(|(p, k)| (p.id().to_owned(), json!(k)))
            .collect();
        json!({
            "settings": s,
            "origin": origin,
            "effective": {
                "transcribe": effective(Role::Transcribe),
                "summarise": effective(Role::Summarise),
                "saveFolderPath": self.storage_root.join(&s.save_folder),
                "defaultTemplate": self.effective_default(&s),
            },
            "keys": keys_json,
        })
        .to_string()
    }

    /// `nativeSetSettings`: merge patch, validate, save atomically, then reply as
    /// `nativeGetSettings`. A rejected patch changes nothing: `{error, field}`.
    pub fn set_settings_json(&self, patch: &str) -> String {
        let Ok(patch) = serde_json::from_str::<Value>(patch) else {
            return json!({"error": "settings must be JSON", "field": null}).to_string();
        };
        // F10-FR6: the default must be one of the effective templates (a malformed id is
        // reported by `settings::apply`).
        if let Some(t) = patch.get("defaultTemplate").and_then(Value::as_str)
            && settings::is_template_id(t)
            && !self.template_usable(t)
        {
            return json!({"error": "template not found", "field": "defaultTemplate"}).to_string();
        }
        let ks = |p| settings::key_status(&self.core.key_dir, p);
        let secrets: Vec<String> = Provider::ALL
            .iter()
            .filter_map(|p| crate::keyfile::load(&self.core.key_dir, *p).ok())
            .map(|k| k.expose().to_owned())
            .collect();
        let ctx = Ctx {
            storage_root: &self.storage_root,
            key_status: &ks,
            secrets: &secrets,
        };
        let r = self.commit_settings(|cur| {
            settings::apply(cur, &patch, &ctx)
                .map_err(|e| json!({"error": e.message, "field": e.field}).to_string())
        });
        match r {
            Ok(()) => self.get_settings_json(),
            Err(e) => e,
        }
    }

    /// Replace the settings with `f(current)` and save them atomically. `Err` is the reply to
    /// return: `f`'s own, or why settings.json can't be changed now (nothing is written).
    pub(crate) fn commit_settings(
        &self,
        f: impl FnOnce(&Settings) -> Result<Settings, String>,
    ) -> Result<(), String> {
        self.settings(); // an unreadable file is tried again first
        let mut st = self.state();
        if st.origin == Origin::Newer {
            let e =
                "settings.json is from a newer version of Meetings, so it can't be changed here";
            return Err(json!({"error": e, "field": null}).to_string());
        }
        if st.origin == Origin::Unreadable {
            let e = "settings.json can't be read just now, so it can't be changed; try again";
            return Err(json!({"error": e, "field": null}).to_string());
        }
        let next = f(&st.settings)?;
        if let Err(e) = settings::save(&settings::path_in(&self.core.app_data_dir), &next) {
            return Err(error_json(&format!("can't save settings: {}", e.kind())));
        }
        (st.settings, st.origin) = (next, Origin::File);
        Ok(())
    }

    /// `nativeTestConnection`. Settings' model override applies only when `provider` is the
    /// role's effective provider: testing another provider checks its own key file's model.
    /// With no transcription model chosen (no override, no `transcribe_model`), it names the
    /// one a job would pick from the listing (D2), and keeps the listing.
    pub fn test_connection(&self, role: &str, provider: &str) -> String {
        let (r, p) = match (Role::from_id(role), Provider::from_id(provider)) {
            (Some(r), Some(p)) => (r, p),
            _ => return api::test_connection_model(&self.core, role, provider, None),
        };
        let (s, _) = self.settings();
        let usable = |q| settings::key_status(&self.core.key_dir, q).usable();
        let model = (settings::effective_provider(&s, r, usable) == p)
            .then(|| s.role(r).model.clone())
            .flatten();
        if r == Role::Transcribe
            && r.providers().contains(&p)
            && model.is_none()
            && let Ok(key) = crate::keyfile::load(&self.core.key_dir, p)
            && key.transcribe_model.is_none()
        {
            return api::test_connection_auto(&self.core, &key, |ids| self.store_models(p, ids));
        }
        api::test_connection_model(&self.core, role, provider, model.as_deref())
    }
}

fn ok() -> Value {
    json!({"ok": true})
}

/// A library result as JSON: the value, or `{"error": ...}`.
fn reply(r: Result<Value, crate::library::LibError>) -> String {
    match r {
        Ok(v) => v.to_string(),
        Err(e) => error_json(&e.to_string()),
    }
}
