//! The JNI-shaped API: strings in, JSON out, never a panic, never a key. `jni.rs` only marshals.
use crate::keyfile::{self, KeyError, Provider};
use crate::providers::{self, Endpoints};
use serde_json::json;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

/// Where the core reads keys and sends requests. Tests build their own; JNI uses `Core::device`.
#[derive(Clone, Debug)]
pub struct Core {
    pub app_data_dir: PathBuf,
    pub key_dir: PathBuf,
    pub endpoints: Endpoints,
    pub timeout: Duration,
    pub https_only: bool,
}

impl Core {
    /// The on-device configuration: sn-copilot's key folder, the real hosts, HTTPS only.
    pub fn device(app_data_dir: &str) -> Core {
        Core {
            app_data_dir: app_data_dir.into(),
            key_dir: keyfile::DEFAULT_KEY_DIR.into(),
            endpoints: Endpoints::default(),
            timeout: providers::TEST_TIMEOUT,
            https_only: true,
        }
    }
}

static CORE: OnceLock<Core> = OnceLock::new();

/// OpenAI and Gemini transcription defaults (F4-FR3).
const OPENAI_TRANSCRIBE: &str = "gpt-transcribe";
const GEMINI_TRANSCRIBE: &str = "gemini-3.8-flash";

/// Run `f`, turning a panic into `{"error": "internal error"}` (INV6, NFR4).
pub fn guarded(f: impl FnOnce() -> String) -> String {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| error_json("internal error"))
}

/// The error envelope every export returns instead of throwing.
pub fn error_json(msg: &str) -> String {
    json!({ "error": msg }).to_string()
}

/// `nativeVersion`: the crate version, as a plain string (F1-FR5).
pub fn version_json() -> String {
    crate::version().to_owned()
}

/// `nativeInit(appDataDir)`: build the runtime and remember the app data dir. Idempotent: a
/// second call keeps the first directory.
pub fn init(app_data_dir: &str) -> String {
    guarded(|| {
        if app_data_dir.is_empty() || !app_data_dir.starts_with('/') {
            return error_json("appDataDir must be an absolute path");
        }
        if let Err(e) = crate::runtime::get() {
            return error_json(&format!("runtime: {e}"));
        }
        let core = CORE.get_or_init(|| Core::device(app_data_dir));
        json!({ "ok": true, "version": crate::version(), "appDataDir": core.app_data_dir })
            .to_string()
    })
}

/// `nativeTestConnection(role, provider)`; blocking, so call it off the UI thread.
pub fn test_connection(role: &str, provider: &str) -> String {
    guarded(|| match CORE.get() {
        Some(core) => test_connection_with(core, role, provider),
        None => error_json("nativeInit has not run"),
    })
}

/// `test_connection` against an explicit `Core`. Must not be called on a runtime thread.
pub fn test_connection_with(core: &Core, role: &str, provider: &str) -> String {
    let Some(p) = Provider::from_id(provider) else {
        return error_json("unknown provider");
    };
    let transcribe_default = match (role, p) {
        ("summarise", _) => None,
        ("transcribe", Provider::Anthropic) => return error_json("anthropic cannot transcribe"),
        ("transcribe", Provider::Gemini) => Some(GEMINI_TRANSCRIBE),
        ("transcribe", Provider::OpenAi) => Some(OPENAI_TRANSCRIBE),
        _ => return error_json("unknown role"),
    };
    let key = match keyfile::load(&core.key_dir, p) {
        Ok(k) => k,
        Err(e) => return key_error_json(p, &e),
    };
    let model = match transcribe_default {
        None => key.model.clone(),
        Some(default) => key.transcribe_model.clone().unwrap_or(default.to_owned()),
    };
    let http = match providers::http_client(core.timeout, core.https_only) {
        Ok(c) => c,
        Err(e) => return error_json(&crate::scrub::scrub(&e, &[key.expose()])),
    };
    let fut = providers::test_connection(&http, &core.endpoints, &key, &model, core.timeout);
    match crate::runtime::block_on(fut) {
        Ok(r) => json!(r).to_string(),
        Err(e) => error_json(&format!("runtime: {e}")),
    }
}

/// No usable key: the F4-FR5 message, in the same shape as a connection result.
fn key_error_json(p: Provider, e: &KeyError) -> String {
    let file = p.canonical_file_name();
    let (status, detail) = match e {
        KeyError::NotFound {
            vault_present: true,
            ..
        } => (
            "no_key",
            format!(
                "Your keys are in SN-Copilot's encrypted vault; Meetings v1 needs the plaintext {file}"
            ),
        ),
        KeyError::NotFound { .. } => ("no_key", format!("Add {file} to MyStyle/SnCopilot")),
        // Scrubbed as well: KeyError text comes from the user's file (INV2, F4-FR6).
        other => (
            "key_file_error",
            crate::scrub::scrub(&format!("{} key file: {other}", p.id()), &[]),
        ),
    };
    json!({
        "ok": false, "status": status, "ms": 0, "provider": p.id(),
        "model": null, "model_found": null, "detail": detail,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_the_error_envelope() {
        assert_eq!(guarded(|| panic!("boom")), r#"{"error":"internal error"}"#);
        assert_eq!(guarded(|| "fine".to_owned()), "fine");
    }

    #[test]
    fn error_json_escapes() {
        assert_eq!(error_json("a\"b\n"), r#"{"error":"a\"b\n"}"#);
    }

    #[test]
    fn version_json_is_the_version() {
        assert_eq!(version_json(), crate::version());
        assert_eq!(crate::version(), "0.1.0");
    }

    #[test]
    fn the_device_core_uses_the_real_hosts_over_https() {
        let c = Core::device("/data/x");
        assert_eq!(c.key_dir, PathBuf::from(keyfile::DEFAULT_KEY_DIR));
        assert_eq!(c.endpoints, Endpoints::default());
        assert_eq!(c.timeout, providers::TEST_TIMEOUT);
        assert!(c.https_only);
    }
}
