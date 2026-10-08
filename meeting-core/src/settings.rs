//! F10 core side: `settings.json` in app-private storage. The core owns it; the shell renders it.
use crate::keyfile::{self, KeyError, Provider, ProviderKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};

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
/// Folders under the storage root that must never receive meeting files (F10-FR3), lowercase.
const FORBIDDEN: [&[&str]; 2] = [
    &["mystyle", "sncopilot"],
    &["mystyle", "meetings", "templates"],
];
/// F10-AC4's exact text.
pub const OUTSIDE: &str = "must be inside internal storage";

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
        next.save_folder = validate_save_folder(ctx.storage_root, &next.save_folder)?;
        next.previous_folders = remember(current, &next.save_folder);
    }
    Ok(next)
}

/// The previous folders after moving to `new`: the old current folder first, then the older
/// ones, without `new` itself and without repeats. Shared storage ignores case, so do we.
fn remember(current: &Settings, new: &str) -> Vec<String> {
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for f in std::iter::once(&current.save_folder).chain(&current.previous_folders) {
        if !same(f, new) && !out.iter().any(|o| same(o, f)) {
            out.push(f.clone());
        }
    }
    out
}

/// F10-FR3: `input` relative to `root` (or absolute under it) → the normalised relative path.
/// The folder must resolve inside `root`, also through symlinks, must not be `root` itself or
/// inside MyStyle/SnCopilot or MyStyle/Meetings/templates, and must be creatable and
/// writable. It is created here.
pub fn validate_save_folder(root: &Path, input: &str) -> Result<String, SettingsError> {
    const F: &str = "saveFolder";
    let outside = || err(F, OUTSIDE);
    let trimmed = input.trim();
    let root_s = root.to_string_lossy();
    let rel = match trimmed.strip_prefix(root_s.as_ref()) {
        Some(r) if r.is_empty() || r.starts_with('/') => r,
        _ if trimmed.starts_with('/') => return Err(outside()),
        _ => trimmed,
    };
    if rel.chars().any(|c| c.is_control() || c == '\\') {
        return Err(err(F, "folder name has invalid characters"));
    }
    let mut parts: Vec<&str> = Vec::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str().ok_or_else(outside)?),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => return Err(outside()),
        }
    }
    if parts.is_empty() {
        return Err(err(
            F,
            "choose a folder inside internal storage, not its top level",
        ));
    }
    check_forbidden(&parts)?;
    let canon_root = root
        .canonicalize()
        .map_err(|_| err(F, "internal storage is not available"))?;
    let inside = |p: &Path| p.canonicalize().is_ok_and(|c| c.starts_with(&canon_root));
    // Symlinks: the deepest part that exists must already resolve inside the root.
    let full = parts.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    let mut probe = full.as_path();
    while !probe.exists() {
        probe = probe.parent().ok_or_else(outside)?;
    }
    if !inside(probe) {
        return Err(outside());
    }
    std::fs::create_dir_all(&full)
        .map_err(|e| err(F, format!("can't create the folder: {}", e.kind())))?;
    // ... and so must the folder itself, now that it exists (a link to a forbidden folder).
    let canon = full.canonicalize().map_err(|_| outside())?;
    let canon_parts: Vec<&str> = match canon.strip_prefix(&canon_root) {
        Ok(r) => r.iter().filter_map(|c| c.to_str()).collect(),
        Err(_) => return Err(outside()),
    };
    if canon_parts.is_empty() {
        return Err(outside());
    }
    check_forbidden(&canon_parts)?;
    let test = full.join(".sn-meeting-write-test");
    std::fs::File::create(&test)
        .and_then(|_| std::fs::remove_file(&test))
        .map_err(|e| err(F, format!("the folder isn't writable: {}", e.kind())))?;
    Ok(parts.join("/"))
}

/// Case-insensitive, as shared storage is.
fn check_forbidden(parts: &[&str]) -> Result<(), SettingsError> {
    let lower: Vec<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    for f in FORBIDDEN {
        if lower.len() >= f.len() && lower.iter().zip(f).all(|(a, b)| a == b) {
            return Err(err(
                "saveFolder",
                "can't save into MyStyle/SnCopilot or MyStyle/Meetings/templates",
            ));
        }
    }
    Ok(())
}

impl Settings {
    /// Every library folder, current first, as absolute paths under `root` (F10-FR3).
    pub fn folders(&self, root: &Path) -> Vec<PathBuf> {
        std::iter::once(&self.save_folder)
            .chain(&self.previous_folders)
            .map(|f| root.join(f))
            .collect()
    }
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
