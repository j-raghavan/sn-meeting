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

mod folder;
mod keys;
pub use folder::{OUTSIDE, validate_save_folder};
pub use keys::{
    KeyStatus, ModelSource, Role, effective_model, effective_provider, key_status, key_status_of,
};

/// A settings file is a few hundred bytes; anything this big is not one.
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;
const MAX_MODEL_LEN: usize = 128;

/// Fields this version doesn't know, kept on rewrite at every level (F10-FR7, as F3-FR6).
/// Their values are kept; how a number is written may change (`1e2` becomes `100.0`).
pub type Extra = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    /// F10-FR2 / F2-FR8. A bad value in the file reads as `auto` rather than discarding the
    /// whole file; a bad value in a patch is an error.
    #[serde(deserialize_with = "lenient_mic")]
    pub mic: MicChoice,
    /// Relative to the storage root, `/`-separated (F10-FR3).
    pub save_folder: String,
    /// Earlier save folders, newest first; the library keeps listing them. Core-maintained.
    pub previous_folders: Vec<String>,
    pub transcribe: RoleSettings,
    pub summarise: RoleSettings,
    /// F10-FR6: a template id ([`is_template_id`]); `App` checks a new choice against the
    /// effective templates. A bad id in the file reads as the default.
    #[serde(deserialize_with = "lenient_template")]
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
            mic: MicChoice::Auto,
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

/// The remembered mic: `"auto"` (USB first, then Bluetooth), `"usb:<device name>"` or
/// `"bt:<device name>"`, on disk and over JNI (F10-FR2, F2-FR8). Chosen by device name.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum MicChoice {
    #[default]
    Auto,
    Usb(String),
    Bluetooth(String),
}

const MAX_MIC_NAME: usize = 200;

impl TryFrom<String> for MicChoice {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let named = |n: &str| {
            let n = n.trim();
            (!n.is_empty() && n.chars().count() <= MAX_MIC_NAME && !n.chars().any(char::is_control))
                .then(|| n.to_owned())
                .ok_or_else(|| "mic name is invalid".to_owned())
        };
        if s == "auto" {
            Ok(MicChoice::Auto)
        } else if let Some(n) = s.strip_prefix("usb:") {
            Ok(MicChoice::Usb(named(n)?))
        } else if let Some(n) = s.strip_prefix("bt:") {
            Ok(MicChoice::Bluetooth(named(n)?))
        } else {
            Err("mic must be auto, usb:<name> or bt:<name>".to_owned())
        }
    }
}

impl From<MicChoice> for String {
    fn from(m: MicChoice) -> String {
        match m {
            MicChoice::Auto => "auto".into(),
            MicChoice::Usb(n) => format!("usb:{n}"),
            MicChoice::Bluetooth(n) => format!("bt:{n}"),
        }
    }
}

fn lenient_mic<'de, D: serde::Deserializer<'de>>(d: D) -> Result<MicChoice, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_str()
        .and_then(|s| MicChoice::try_from(s.to_owned()).ok())
        .unwrap_or_default())
}

/// A template id: kebab-case (F11-FR1), 1-64 characters of `a-z`, `0-9` and single inner
/// hyphens. Also the user copy's file stem, so it can never name a path.
pub fn is_template_id(id: &str) -> bool {
    id.len() <= 64
        && !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn lenient_template<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_str()
        .filter(|s| is_template_id(s))
        .unwrap_or(DEFAULT_TEMPLATE)
        .to_owned())
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
    /// Written by a newer app (a higher `schema`): kept untouched, defaults in use, and not
    /// saved over until that app's version is back (as a newer sidecar is refused).
    Newer,
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
        Ok(bytes) if newer(&bytes) => (Settings::default(), Origin::Newer),
        Ok(bytes) => match lenient(&bytes) {
            Some(s) => (sanitised(s), Origin::File),
            None => (Settings::default(), keep_bad(path)),
        },
        Err(_) => (Settings::default(), keep_bad(path)),
    }
}

/// The file's settings, field by field: a value of the wrong type (a hand edit) reads as that
/// field's default and every other field is kept, so one bad value can't lose the save folders.
/// `None` when it isn't a settings file at all: not JSON, not an object, or a `schema` that
/// isn't a number.
fn lenient(bytes: &[u8]) -> Option<Settings> {
    let Ok(Value::Object(file)) = serde_json::from_slice::<Value>(bytes) else {
        return None;
    };
    if file.get("schema").is_some_and(|v| !v.is_u64()) {
        return None;
    }
    let mut acc = serde_json::to_value(Settings::default()).ok()?;
    for (k, v) in &file {
        accept(&mut acc, &[k.as_str()], v);
    }
    serde_json::from_value(acc).ok()
}

/// Put `v` at `path` in `acc` if the result still reads as Settings; otherwise try an object's
/// members one by one (a bad `noticeShown.openai` keeps `noticeShown.gemini`).
fn accept(acc: &mut Value, path: &[&str], v: &Value) {
    let mut trial = acc.clone();
    let mut slot = &mut trial;
    for k in path {
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        slot = slot
            .as_object_mut()
            .expect("made an object above")
            .entry((*k).to_owned())
            .or_insert(Value::Null);
    }
    *slot = v.clone();
    if serde_json::from_value::<Settings>(trial.clone()).is_ok() {
        *acc = trial;
    } else if let Value::Object(members) = v {
        for (k, m) in members {
            let mut sub = path.to_vec();
            sub.push(k);
            accept(acc, &sub, m);
        }
    }
}

/// Whether the file declares a schema newer than this app's.
fn newer(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| v["schema"].as_u64())
        .is_some_and(|n| n > u64::from(SCHEMA))
}

/// A hand-edited file may hold values a patch could never set: each such field reads as its
/// default (an escaping or forbidden save folder, a role provider that can't do the role, a
/// malformed model), so it can neither point the library outside storage nor block patches.
fn sanitised(mut s: Settings) -> Settings {
    if !folder::plausible(&s.save_folder) {
        s.save_folder = DEFAULT_SAVE_FOLDER.into();
    }
    s.previous_folders.retain(|f| folder::plausible(f));
    for role in Role::ALL {
        let rs = s.role_mut(role);
        if rs.provider.is_some_and(|p| !role.providers().contains(&p)) {
            rs.provider = None;
        }
        if rs.model.as_deref().is_some_and(|m| !model_ok(m)) {
            rs.model = None;
        }
    }
    s
}

fn model_ok(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= MAX_MODEL_LEN
        && !m.chars().any(|c| c.is_whitespace() || c.is_control())
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
    /// Every key value the key files hold: a model override may not contain one (INV2).
    pub secrets: &'a [String],
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
    if let Some(t) = patch.get("defaultTemplate").filter(|t| !t.is_null())
        && !t.as_str().is_some_and(is_template_id)
    {
        return Err(err("defaultTemplate", "invalid template id"));
    }
    if let Some(m) = patch.get("mic").filter(|m| !m.is_null()) {
        let m = m
            .as_str()
            .ok_or_else(|| err("mic", "mic must be a string"))?;
        MicChoice::try_from(m.to_owned()).map_err(|e| err("mic", e))?;
    }
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
        next.previous_folders = folder::remember(current, &next.save_folder);
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
    // Only a changed role is checked, so a value that is already stored never blocks a patch
    // to something else.
    if next.role(role) == current.role(role) {
        return Ok(());
    }
    let rs = next.role_mut(role);
    rs.model = rs
        .model
        .take()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    if let Some(m) = &rs.model
        && !model_ok(m)
    {
        return Err(err(mf, "model name is invalid"));
    }
    // A key (or a cut of one, by scrub's rule) pasted into the model field would be stored
    // and shown; never echo it.
    let secrets: Vec<&str> = ctx.secrets.iter().map(String::as_str).collect();
    if let Some(m) = &rs.model
        && crate::scrub::contains_secret(m, &secrets)
    {
        return Err(err(mf, "that is an API key, not a model name"));
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
