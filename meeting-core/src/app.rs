//! Process state behind the JNI API: `Core` (M1, immutable) plus the settings in use.
use crate::api::{self, Core};
use crate::settings::{self, Origin, Settings};
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

    /// `nativeTestConnection`.
    pub fn test_connection(&self, role: &str, provider: &str) -> String {
        api::test_connection_with(&self.core, role, provider)
    }
}
