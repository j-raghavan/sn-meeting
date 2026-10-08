//! F10-FR4 / F4-FR3 / F4-FR5: roles, each provider's key status, and what will actually run.
use super::{RoleSettings, Settings};
use crate::keyfile::{self, KeyError, Provider, ProviderKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

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

    pub(super) fn role_mut(&mut self, r: Role) -> &mut RoleSettings {
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
