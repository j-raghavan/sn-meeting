//! F10 core side: `settings.json` in app-private storage. The core owns it; the shell renders it.
use crate::keyfile::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "settings.json";
pub const BAD_SUFFIX: &str = ".bad";
/// Android's shared storage; save folders are relative to it (F10-FR3).
pub const STORAGE_ROOT: &str = "/storage/emulated/0";
pub const DEFAULT_SAVE_FOLDER: &str = "EXPORT/sn-meeting";
pub const DEFAULT_TEMPLATE: &str = "general";
/// A settings file is a few hundred bytes; anything this big is not one.
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

/// Fields this version doesn't know, kept on rewrite at every level (F10-FR7, as F3-FR6).
pub type Extra = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    /// F10-FR2 / F2-FR8.
    pub mic: String,
    /// Relative to the storage root, `/`-separated (F10-FR3).
    pub save_folder: String,
    /// Earlier save folders, newest first; the library keeps listing them. Core-maintained.
    pub previous_folders: Vec<String>,
    pub transcribe: RoleSettings,
    pub summarise: RoleSettings,
    /// F10-FR6.
    pub default_template: String,
    /// Security: the one-time "audio is sent to <provider>" notice, per provider.
    pub notice_shown: NoticeShown,
    pub debug: DebugSettings,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA,
            mic: "auto".into(),
            save_folder: DEFAULT_SAVE_FOLDER.into(),
            previous_folders: Vec::new(),
            transcribe: RoleSettings::default(),
            summarise: RoleSettings::default(),
            default_template: DEFAULT_TEMPLATE.into(),
            notice_shown: NoticeShown::default(),
            debug: DebugSettings::default(),
            extra: Extra::new(),
        }
    }
}

/// One role's choice. `provider: null` = "first provider with a key", resolved on every read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleSettings {
    pub provider: Option<Provider>,
    /// `null` = the key file's model / the provider default (F4-FR3).
    pub model: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoticeShown {
    pub openai: bool,
    pub gemini: bool,
    pub anthropic: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DebugSettings {
    /// Segment length override for F2-AC3; `segment::plan` clamps it.
    pub segment_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Where the settings in use came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    File,
    /// No file yet (fresh install): defaults, nothing written until the first change.
    Defaults,
    /// The file was corrupt or unreadable: kept as `settings.json.bad`, defaults in use.
    Recovered,
}

pub fn path_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(FILE_NAME)
}

/// Never fails: a missing or corrupt file gives defaults (ADR D10).
pub fn load(path: &Path) -> (Settings, Origin) {
    match crate::atomic::read_capped(path, MAX_SETTINGS_BYTES) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (Settings::default(), Origin::Defaults)
        }
        Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
            Ok(s) => (s, Origin::File),
            Err(_) => (Settings::default(), keep_bad(path)),
        },
        Err(_) => (Settings::default(), keep_bad(path)),
    }
}

/// Move the bad file aside for diagnosis; the next save writes a fresh one.
fn keep_bad(path: &Path) -> Origin {
    let mut bad = path.as_os_str().to_owned();
    bad.push(BAD_SUFFIX);
    let _ = std::fs::rename(path, PathBuf::from(bad));
    Origin::Recovered
}

pub fn save(path: &Path, s: &Settings) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(path, &bytes)
}
