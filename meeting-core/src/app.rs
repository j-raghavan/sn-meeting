//! Process state behind the JNI API: `Core` (M1, immutable) plus the settings in use.
use crate::api::{self, Core, error_json};
use crate::keyfile::Provider;
use crate::settings::{self, Ctx, KeyStatus, Origin, Role, Settings};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Debug)]
pub struct App {
    pub core: Core,
    /// `/storage/emulated/0` on the device; a tempdir in tests.
    pub storage_root: PathBuf,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    settings: Settings,
    origin: Origin,
}

impl App {
    /// Load `settings.json` from `core.app_data_dir`. Never fails (ADR D10).
    pub fn new(core: Core, storage_root: PathBuf) -> App {
        let (settings, origin) = settings::load(&settings::path_in(&core.app_data_dir));
        App {
            core,
            storage_root,
            state: Mutex::new(State { settings, origin }),
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
