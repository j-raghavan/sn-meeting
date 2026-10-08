//! F4: sn-copilot `copilot-key-<provider>.txt` files (port of sn-copilot keyFiles.ts:43-226).
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Where sn-copilot (and so Meetings) reads key files on the device (F4-FR2).
pub const DEFAULT_KEY_DIR: &str = "/storage/emulated/0/MyStyle/SnCopilot";
/// A key file is a few hundred bytes; anything bigger is not one.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// sn-copilot's encrypted vault, which Meetings v1 cannot read (F4-FR5).
const VAULT_FILES: [&str; 2] = ["copilot-key.enc", ".copilot-key.enc"];

/// Serialised as its canonical id (settings.json, sidecars, JSON replies).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAi,
    Gemini,
    Anthropic,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::OpenAi, Provider::Gemini, Provider::Anthropic];

    /// The canonical id: the `provider=` value and the JNI argument.
    pub fn id(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::Gemini => "gemini",
            Provider::Anthropic => "anthropic",
        }
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn canonical_file_name(self) -> String {
        format!("copilot-key-{}.txt", self.id())
    }
}

/// What a filename means (keyFiles.ts:56-79). DeepSeek is recognised and ignored.
#[derive(Debug, PartialEq, Eq)]
pub enum FileMatch {
    Provider(Provider),
    Ignored,
}

/// keyFiles.ts `matchKeyFilename`: case-insensitive, trimmed, `copilot-key-<alias>.txt`.
pub fn match_key_filename(file_name: &str) -> Option<FileMatch> {
    let lower = js_trim(&file_name.to_lowercase()).to_owned();
    let suffix = lower.strip_prefix("copilot-key-")?.strip_suffix(".txt")?;
    Some(match suffix {
        "openai" | "gpt" | "chatgpt" => FileMatch::Provider(Provider::OpenAi),
        "gemini" | "google" | "google-gemini" => FileMatch::Provider(Provider::Gemini),
        "anthropic" | "claude" | "claude-ai" | "claude-anthropic" => {
            FileMatch::Provider(Provider::Anthropic)
        }
        "deepseek" => FileMatch::Ignored,
        _ => return None,
    })
}

/// A parsed key file. The key is private and never printed (`Debug` redacts it, INV2).
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderKey {
    pub provider: Provider,
    pub model: String,
    /// Optional `transcribe_model=` line (F4-FR3); sn-copilot ignores unknown keys.
    pub transcribe_model: Option<String>,
    /// The file it came from; empty for text parsed directly.
    pub source: PathBuf,
    key: String,
}

impl ProviderKey {
    /// The key itself: every call site is a place it can leak (INV2).
    pub fn expose(&self) -> &str {
        &self.key
    }

    /// The bare file name, for "Key found: copilot-key-openai.txt" (F4-FR4).
    pub fn file_name(&self) -> String {
        self.source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl fmt::Debug for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderKey")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("transcribe_model", &self.transcribe_model)
            .field("source", &self.source)
            .field("key", &"[redacted]")
            .finish()
    }
}

/// Why a provider has no usable key. Messages match sn-copilot's parse-error reasons.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("no key file for {provider}")]
    NotFound {
        provider: &'static str,
        /// `copilot-key.enc` / `.copilot-key.enc` exists (F4-FR5).
        vault_present: bool,
    },
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("provider=\"{found}\" mismatches filename suffix (expected canonical \"{expected}\")")]
    ProviderMismatch {
        expected: &'static str,
        found: String,
    },
    #[error("key file unreadable: {0}")]
    Unreadable(String),
}

/// JS `String.prototype.trim`: Rust's whitespace set minus U+0085, plus U+FEFF (the BOM).
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// The `provider=` value as it may appear in an error: only a plausible provider id is echoed.
/// A one-line file (`provider=openai key=sk-… model=m`, or U+2028 separators, which JS trim
/// strips but does not split on) or `provider=<key>` would otherwise put the key in the message.
fn echoable(found: &str) -> String {
    let plausible = (1..=32).contains(&found.len())
        && found.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
    if plausible {
        found.to_owned()
    } else {
        "<invalid>".to_owned()
    }
}

/// F4-FR1: parse one file's text for `provider` (keyFiles.ts:104-162).
pub fn parse(text: &str, provider: Provider) -> Result<ProviderKey, KeyError> {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
    for raw in normalised.split('\n') {
        let line = js_trim(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue; // malformed line: ignored, as in sn-copilot
        };
        let k = js_trim(k);
        if k.is_empty() {
            continue;
        }
        fields.insert(k, js_trim(v)); // duplicate: last wins
    }
    let field = |name: &'static str| {
        fields
            .get(name)
            .copied()
            .filter(|v| !v.is_empty())
            .ok_or(KeyError::MissingField(name))
    };
    let found = field("provider")?;
    if found != provider.id() {
        return Err(KeyError::ProviderMismatch {
            expected: provider.id(),
            found: echoable(found),
        });
    }
    let key = field("key")?;
    let model = field("model")?;
    Ok(ProviderKey {
        provider,
        model: model.to_owned(),
        transcribe_model: field("transcribe_model").ok().map(str::to_owned),
        source: PathBuf::new(),
        key: key.to_owned(),
    })
}

/// F4-FR2: find and parse `provider`'s file in `dir`. The canonical name wins over aliases;
/// among aliases the lexically first name wins; a file that fails to read or parse gives way
/// to the next. DeepSeek files and directories are skipped.
pub fn load(dir: &Path, provider: Provider) -> Result<ProviderKey, KeyError> {
    let not_found = || KeyError::NotFound {
        provider: provider.id(),
        vault_present: VAULT_FILES.iter().any(|v| dir.join(v).is_file()),
    };
    // A missing folder means "no key yet"; any other failure (all-files access not granted)
    // must not tell the user to add a file that is already there.
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(KeyError::Unreadable(e.kind().to_string())),
    };
    let canonical = provider.canonical_file_name();
    let mut candidates: Vec<(bool, String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let matched = match_key_filename(&name) == Some(FileMatch::Provider(provider));
            let alias = js_trim(&name.to_lowercase()) != canonical;
            matched.then(|| (alias, name, e.path()))
        })
        .collect();
    candidates.sort();
    // sn-copilot parses every file and keeps the good ones, so a broken file never hides a
    // working alias. If none parses, report the first one's error (the canonical file's, when
    // there is one).
    let mut first_error = None;
    for (_, _, path) in candidates {
        match read_capped(&path).and_then(|text| parse(&text, provider)) {
            Ok(mut key) => {
                key.source = path;
                return Ok(key);
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(not_found))
}

/// Read at most `MAX_FILE_BYTES`, decoding lossily as sn-copilot's `decodeUtf8` does. Errors
/// carry the I/O kind only, never the path or contents.
fn read_capped(path: &Path) -> Result<String, KeyError> {
    let unreadable = |e: std::io::Error| KeyError::Unreadable(e.kind().to_string());
    if std::fs::metadata(path).map_err(unreadable)?.len() > MAX_FILE_BYTES {
        return Err(KeyError::Unreadable("file too large".into()));
    }
    let bytes = std::fs::read(path).map_err(unreadable)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
