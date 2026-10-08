//! Process state behind the JNI API: `Core` (M1, immutable), the settings in use, the device
//! time zone and what is live (recording, jobs).
use crate::api::{self, Core, error_json};
use crate::keyfile::Provider;
use crate::library::{Library, Liveness};
use crate::settings::{self, Ctx, KeyStatus, Origin, Role, Settings};
use crate::sidecar::{Audio, Mic};
use crate::tz::{DeviceTz, LocalTz};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Debug)]
pub struct App {
    pub core: Core,
    /// `/storage/emulated/0` on the device; a tempdir in tests.
    pub storage_root: PathBuf,
    pub tz: Arc<dyn LocalTz>,
    state: Mutex<State>,
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
        App {
            core,
            storage_root,
            tz,
            state: Mutex::new(State {
                settings,
                origin,
                live: Liveness::default(),
            }),
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

    pub fn settings(&self) -> (Settings, Origin) {
        let st = self.state();
        (st.settings.clone(), st.origin)
    }

    /// What is live right now.
    pub fn liveness(&self) -> Liveness {
        self.state().live.clone()
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
        reply(
            self.library()
                .close_segment(id, index, duration_ms)
                .map(|()| ok()),
        )
    }

    /// `nativeFinishMeeting`; an empty `note` means none. The recording is over either way,
    /// so the meeting stops being live even if the sidecar can't be updated.
    pub fn finish_json(&self, id: &str, duration_ms: i64, note: &str) -> String {
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

    /// `nativeSetTodoDone` (F6-FR2).
    pub fn set_todo_done_json(&self, id: &str, todo_id: &str, done: bool) -> String {
        reply(
            self.library()
                .set_todo_done(id, todo_id, done)
                .map(|()| ok()),
        )
    }

    /// `nativeRenameMeeting` (F7-FR5); an empty title clears it.
    pub fn rename_json(&self, id: &str, title: &str) -> String {
        reply(self.library().rename(id, title).map(|()| ok()))
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
            let m = settings::effective_model(role, p, s.role(role).model.as_deref(), from_key);
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
        {
            let mut st = self.state();
            let ks = |p| settings::key_status(&self.core.key_dir, p);
            let ctx = Ctx {
                storage_root: &self.storage_root,
                key_status: &ks,
            };
            let next = match settings::apply(&st.settings, &patch, &ctx) {
                Ok(n) => n,
                Err(e) => return json!({"error": e.message, "field": e.field}).to_string(),
            };
            if let Err(e) = settings::save(&settings::path_in(&self.core.app_data_dir), &next) {
                return error_json(&format!("can't save settings: {}", e.kind()));
            }
            (st.settings, st.origin) = (next, Origin::File);
        }
        self.get_settings_json()
    }

    /// `nativeTestConnection`. Settings' model override applies only when `provider` is the
    /// role's effective provider: testing another provider checks its own key file's model.
    pub fn test_connection(&self, role: &str, provider: &str) -> String {
        let model = match (Role::from_id(role), Provider::from_id(provider)) {
            (Some(r), Some(p)) => {
                let (s, _) = self.settings();
                let usable = |q| settings::key_status(&self.core.key_dir, q).usable();
                (settings::effective_provider(&s, r, usable) == p)
                    .then(|| s.role(r).model.clone())
                    .flatten()
            }
            _ => None,
        };
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
