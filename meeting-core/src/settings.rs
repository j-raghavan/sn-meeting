//! F10 core side: `settings.json` in app-private storage. The core owns it; the shell renders it.
use crate::keyfile::{self, KeyError, Provider, ProviderKey};
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
const MAX_MODEL_LEN: usize = 128;

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

// ---- roles, key status and effective choices (F10-FR4, F4-FR3/FR5) ---------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Transcribe,
    Summarise,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Transcribe, Role::Summarise];

    pub fn from_id(id: &str) -> Option<Role> {
        match id {
            "transcribe" => Some(Role::Transcribe),
            "summarise" => Some(Role::Summarise),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Role::Transcribe => "transcribe",
            Role::Summarise => "summarise",
        }
    }

    /// Selectable providers, in default-preference order (F10-FR4). Anthropic takes no audio.
    pub fn providers(self) -> &'static [Provider] {
        match self {
            Role::Transcribe => &[Provider::OpenAi, Provider::Gemini],
            Role::Summarise => &[Provider::OpenAi, Provider::Gemini, Provider::Anthropic],
        }
    }

    /// The code default (F4-FR3). Summarise has none: the key file's `model` is required.
    pub fn default_model(self, p: Provider) -> Option<&'static str> {
        match (self, p) {
            (Role::Transcribe, Provider::OpenAi) => Some("gpt-transcribe"),
            (Role::Transcribe, Provider::Gemini) => Some("gemini-3.8-flash"),
            _ => None,
        }
    }
}

impl Settings {
    pub fn role(&self, r: Role) -> &RoleSettings {
        match r {
            Role::Transcribe => &self.transcribe,
            Role::Summarise => &self.summarise,
        }
    }

    fn role_mut(&mut self, r: Role) -> &mut RoleSettings {
        match r {
            Role::Transcribe => &mut self.transcribe,
            Role::Summarise => &mut self.summarise,
        }
    }
}

/// A provider's key file as the UI sees it. Never holds the key (INV2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum KeyStatus {
    Found {
        file: String,
        model: String,
        #[serde(rename = "transcribeModel")]
        transcribe_model: Option<String>,
    },
    Missing {
        file: String,
        detail: String,
    },
    /// Only sn-copilot's encrypted vault is there (F4-FR5).
    Vault {
        file: String,
        detail: String,
    },
    /// A key file that does not parse (missing field, provider mismatch).
    Invalid {
        file: String,
        detail: String,
    },
    /// The folder or file can't be read (all-files access revoked, EC9).
    Unreadable {
        detail: String,
    },
}

impl KeyStatus {
    pub fn usable(&self) -> bool {
        matches!(self, KeyStatus::Found { .. })
    }

    /// Why the provider can't be used; `None` when it can.
    pub fn detail(&self) -> Option<&str> {
        match self {
            KeyStatus::Found { .. } => None,
            KeyStatus::Missing { detail, .. }
            | KeyStatus::Vault { detail, .. }
            | KeyStatus::Invalid { detail, .. }
            | KeyStatus::Unreadable { detail } => Some(detail),
        }
    }
}

/// Map a load result to a status, with the same detail strings as Test connection's no-key
/// replies. Text from the user's file is scrubbed, with the key itself when there is one.
pub fn key_status_of(p: Provider, r: &Result<ProviderKey, KeyError>) -> KeyStatus {
    let file = p.canonical_file_name();
    match r {
        Ok(k) => {
            let clean = |m: &str| crate::scrub::scrub(m, &[k.expose()]);
            KeyStatus::Found {
                file: k.file_name(),
                model: clean(&k.model),
                transcribe_model: k.transcribe_model.as_deref().map(clean),
            }
        }
        Err(KeyError::NotFound {
            vault_present: true,
            ..
        }) => KeyStatus::Vault {
            detail: format!(
                "Your keys are in SN-Copilot's encrypted vault; Meetings v1 needs the plaintext {file}"
            ),
            file,
        },
        Err(KeyError::NotFound { .. }) => KeyStatus::Missing {
            detail: format!("Add {file} to MyStyle/SnCopilot"),
            file,
        },
        Err(e) => {
            let detail = crate::scrub::scrub(&format!("{} key file: {e}", p.id()), &[]);
            match e {
                KeyError::Unreadable(_) => KeyStatus::Unreadable { detail },
                _ => KeyStatus::Invalid { file, detail },
            }
        }
    }
}

pub fn key_status(key_dir: &Path, p: Provider) -> KeyStatus {
    key_status_of(p, &keyfile::load(key_dir, p))
}

/// F10-FR4: the stored choice, else the first provider with a usable key, else OpenAI.
pub fn effective_provider(s: &Settings, role: Role, usable: impl Fn(Provider) -> bool) -> Provider {
    s.role(role)
        .provider
        .filter(|p| role.providers().contains(p))
        .or_else(|| role.providers().iter().copied().find(|p| usable(*p)))
        .unwrap_or(Provider::OpenAi)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelSource {
    Override,
    KeyFile,
    Default,
}

/// F4-FR3: override > key file (`transcribe_model` for transcribe, `model` for summarise) >
/// code default. `None` only for summarise without a key.
pub fn effective_model(
    role: Role,
    p: Provider,
    override_model: Option<&str>,
    key: Option<(&str, Option<&str>)>, // (model, transcribe_model) from the key file
) -> Option<(String, ModelSource)> {
    if let Some(m) = override_model {
        return Some((m.to_owned(), ModelSource::Override));
    }
    let from_key = key.and_then(|(model, tm)| match role {
        Role::Summarise => Some(model),
        Role::Transcribe => tm,
    });
    from_key
        .map(|m| (m.to_owned(), ModelSource::KeyFile))
        .or_else(|| {
            role.default_model(p)
                .map(|m| (m.to_owned(), ModelSource::Default))
        })
}

// ---- nativeSetSettings: merge patch + validation ---------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SettingsError {
    /// The settings key at fault (`saveFolder`, `transcribe.provider`, …), when there is one.
    pub field: Option<&'static str>,
    pub message: String,
}

pub(crate) fn err(field: &'static str, message: impl Into<String>) -> SettingsError {
    SettingsError {
        field: Some(field),
        message: message.into(),
    }
}

/// RFC 7396 JSON merge patch: objects merge, `null` removes, anything else replaces.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

/// What validation needs from outside: where storage is, and each provider's key status.
pub struct Ctx<'a> {
    pub storage_root: &'a Path,
    pub key_status: &'a dyn Fn(Provider) -> KeyStatus,
}

/// Merge-patch `patch` onto `current` and validate. `schema` and `previousFolders` are
/// core-owned: they are ignored in a patch.
pub fn apply(current: &Settings, patch: &Value, ctx: &Ctx<'_>) -> Result<Settings, SettingsError> {
    let Some(obj) = patch.as_object() else {
        return Err(SettingsError {
            field: None,
            message: "settings must be a JSON object".into(),
        });
    };
    let mut patch = obj.clone();
    patch.remove("schema");
    patch.remove("previousFolders");
    let mut v = serde_json::to_value(current).map_err(|e| SettingsError {
        field: None,
        message: e.to_string(),
    })?;
    merge_patch(&mut v, &Value::Object(patch));
    let mut next: Settings = serde_json::from_value(v).map_err(|e| SettingsError {
        field: None,
        message: format!("invalid settings: {e}"),
    })?;
    for role in Role::ALL {
        check_role(current, &mut next, role, ctx)?;
    }
    if next.save_folder != current.save_folder {
        return Err(err("saveFolder", "the save folder can't be changed yet"));
    }
    Ok(next)
}

/// F10-FR4: a role's provider must suit the role, and a newly chosen one must have a usable
/// key. A stored choice whose key has since gone is kept: the check runs only on a change.
fn check_role(
    current: &Settings,
    next: &mut Settings,
    role: Role,
    ctx: &Ctx<'_>,
) -> Result<(), SettingsError> {
    let (pf, mf) = match role {
        Role::Transcribe => ("transcribe.provider", "transcribe.model"),
        Role::Summarise => ("summarise.provider", "summarise.model"),
    };
    let rs = next.role_mut(role);
    rs.model = rs
        .model
        .take()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    if let Some(m) = &rs.model
        && (m.len() > MAX_MODEL_LEN || m.chars().any(|c| c.is_whitespace() || c.is_control()))
    {
        return Err(err(mf, "model name is invalid"));
    }
    let Some(p) = rs.provider else {
        return Ok(());
    };
    if !role.providers().contains(&p) {
        return Err(err(pf, format!("{} cannot {}", p.id(), role.id())));
    }
    if current.role(role).provider != Some(p)
        && let Some(d) = (ctx.key_status)(p).detail()
    {
        return Err(err(pf, d));
    }
    Ok(())
}
