//! The JNI-shaped API: strings in, JSON out, never a panic, never a key. `jni.rs` only marshals.
use crate::app::App;
use crate::keyfile::{self, Provider};
use crate::providers::{self, Endpoints};
use crate::settings::{self, KeyStatus, Role};
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

/// The process's app, created by `nativeInit`. Every other export needs it.
static APP: OnceLock<App> = OnceLock::new();

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

/// `guarded(f(app))`, or the "not initialised" envelope before `nativeInit`.
fn with_app(f: impl FnOnce(&App) -> String) -> String {
    guarded(|| match APP.get() {
        Some(app) => f(app),
        None => error_json("nativeInit has not run"),
    })
}

/// `nativeInit(appDataDir)`: build the runtime and the app, which loads `settings.json` from
/// the app data dir (F10-FR7). Idempotent: a second call keeps the first directory.
pub fn init(app_data_dir: &str) -> String {
    guarded(|| {
        if app_data_dir.is_empty() || !app_data_dir.starts_with('/') {
            return error_json("appDataDir must be an absolute path");
        }
        if let Err(e) = crate::runtime::get() {
            return error_json(&format!("runtime: {e}"));
        }
        let app = APP.get_or_init(|| App::device(app_data_dir));
        json!({ "ok": true, "version": crate::version(), "appDataDir": app.core.app_data_dir })
            .to_string()
    })
}

/// `nativeTestConnection(role, provider)`; blocking, so call it off the UI thread.
pub fn test_connection(role: &str, provider: &str) -> String {
    with_app(|app| app.test_connection(role, provider))
}

/// Test connection checking `model_override` instead of the key file's model when given
/// (F10-FR4: Settings' override for this role and provider).
pub fn test_connection_model(
    core: &Core,
    role: &str,
    provider: &str,
    model_override: Option<&str>,
) -> String {
    let Some(p) = Provider::from_id(provider) else {
        return error_json("unknown provider");
    };
    let Some(r) = Role::from_id(role) else {
        return error_json("unknown role");
    };
    if !r.providers().contains(&p) {
        return error_json(&format!("{} cannot {}", p.id(), r.id()));
    }
    let key = match keyfile::load(&core.key_dir, p) {
        Ok(k) => k,
        Err(e) => return key_error_json(p, &settings::key_status_of(p, &Err(e))),
    };
    let from_key = Some((key.model.as_str(), key.transcribe_model.as_deref()));
    // Some for every usable key: summarise has the key file's model, transcribe a default.
    let Some((model, _)) = settings::effective_model(r, p, model_override, from_key) else {
        return error_json("no model");
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

/// D2: Test connection for a transcription with no model chosen: the listing names the model
/// a job would pick; `keep` receives the listing.
pub(crate) fn test_connection_auto(
    core: &Core,
    key: &crate::keyfile::ProviderKey,
    keep: impl FnOnce(Vec<String>),
) -> String {
    let http = match providers::http_client(core.timeout, core.https_only) {
        Ok(c) => c,
        Err(e) => return error_json(&crate::scrub::scrub(&e, &[key.expose()])),
    };
    let fut = providers::test_connection_listing(&http, &core.endpoints, key, None, core.timeout);
    match crate::runtime::block_on(fut) {
        Ok((r, ids)) => {
            if let Some(ids) = ids {
                keep(ids);
            }
            json!(r).to_string()
        }
        Err(e) => error_json(&format!("runtime: {e}")),
    }
}

/// No usable key: the F4-FR5 message, in the same shape as a connection result.
fn key_error_json(p: Provider, ks: &KeyStatus) -> String {
    let status = match ks {
        KeyStatus::Missing { .. } | KeyStatus::Vault { .. } => "no_key",
        _ => "key_file_error",
    };
    json!({
        "ok": false, "status": status, "ms": 0, "provider": p.id(),
        "model": null, "model_found": null, "detail": ks.detail(),
    })
    .to_string()
}

/// `nativeGetSettings`.
pub fn get_settings() -> String {
    with_app(App::get_settings_json)
}

/// `nativeSetSettings(json)`: an RFC 7396 merge patch.
pub fn set_settings(patch: &str) -> String {
    with_app(|app| app.set_settings_json(patch))
}

/// `nativeBeginMeeting(startedAtMs, micJson, audioJson)` (F3-FR1).
pub fn begin_meeting(started_at_ms: i64, mic_json: &str, audio_json: &str) -> String {
    with_app(|app| app.begin_json(started_at_ms, mic_json, audio_json))
}

/// `nativeAddSegment(id, segPath, startOffsetMs)` (F3-FR2).
pub fn add_segment(id: &str, seg_path: &str, start_offset_ms: i64) -> String {
    with_app(|app| app.add_segment_json(id, seg_path, start_offset_ms))
}

/// `nativeCloseSegment(id, segIndex, durationMs)` (F3-FR2); `segIndex` is 0-based.
pub fn close_segment(id: &str, seg_index: i32, duration_ms: i64) -> String {
    with_app(|app| app.close_segment_json(id, seg_index, duration_ms))
}

/// `nativeListRecent(limit)` (F3-FR3).
pub fn list_recent(limit: i32) -> String {
    with_app(|app| app.list_recent_json(limit))
}

/// `nativeGetMeeting(id)`.
pub fn get_meeting(id: &str) -> String {
    with_app(|app| app.get_meeting_json(id))
}

/// `nativeOpenTodos()` (F6-FR1).
pub fn open_todos() -> String {
    with_app(App::open_todos_json)
}

/// `nativeSetTodoDone(id, todoId, done)` (F6-FR2).
pub fn set_todo_done(id: &str, todo_id: &str, done: bool) -> String {
    with_app(|app| app.set_todo_done_json(id, todo_id, done))
}

/// `nativeRenameMeeting(id, title)` (F7-FR5).
pub fn rename_meeting(id: &str, title: &str) -> String {
    with_app(|app| app.rename_json(id, title))
}

/// `nativeSetMeetingTemplate(id, templateId)` (F7-FR2).
pub fn set_meeting_template(id: &str, template_id: &str) -> String {
    with_app(|app| app.set_template_json(id, template_id))
}

/// `nativeDeleteMeeting(id)` (F7-FR4).
pub fn delete_meeting(id: &str) -> String {
    with_app(|app| app.delete_json(id))
}

/// `nativeRecover(serviceAlive)` (F3-FR5): `{recovered, measure, flagged}`.
pub fn recover(service_alive: bool) -> String {
    with_app(|app| app.recover_json(service_alive))
}

/// `nativeTranscribe(id)` (F5): pending segments, then summarise. Blocking for minutes.
pub fn transcribe(id: &str) -> String {
    with_app(|app| app.run_job_json(id, crate::app::Job::Transcribe))
}

/// `nativeTranscribeAgain(id)` (F5-FR13): every segment again, then summarise. Blocking.
pub fn transcribe_again(id: &str) -> String {
    with_app(|app| app.run_job_json(id, crate::app::Job::TranscribeAgain))
}

/// `nativeSummarise(id)`: "Summarise again" (F5-FR8). Blocking.
pub fn summarise(id: &str) -> String {
    with_app(|app| app.run_job_json(id, crate::app::Job::Summarise))
}

/// `nativeCancel(id)`: `{"cancelled": bool}`; returns at once, from any thread.
pub fn cancel(id: &str) -> String {
    with_app(|app| app.cancel_json(id))
}

/// `nativeListTemplates()` (F11-FR5).
pub fn list_templates() -> String {
    with_app(App::list_templates_json)
}

/// `nativeGetTemplate(id)`: the effective version.
pub fn get_template(id: &str) -> String {
    with_app(|app| app.get_template_json(id))
}

/// `nativeSaveTemplate(json)`: Template | `{error, line, field}`.
pub fn save_template(json: &str) -> String {
    with_app(|app| app.save_template_json(json))
}

/// `nativeResetTemplate(id)`: `{ok, origin}`.
pub fn reset_template(id: &str) -> String {
    with_app(|app| app.reset_template_json(id))
}

/// `nativePreviewTemplate(json)`: `{markdown}` | `{error, line, field}`.
pub fn preview_template(json: &str) -> String {
    with_app(|app| app.preview_template_json(json))
}

/// `nativeUpdateDecide(installedVersion, releaseJson)` (F14-FR5, inkread's JNI `decide`): is a
/// fetched `releases/latest` payload newer than the installed version? Needs no `nativeInit`:
/// the shell fetched the JSON, the core only compares. Junk in gives `{"updateAvailable":false}`.
pub fn update_decide(installed_version: &str, release_json: &str) -> String {
    guarded(|| crate::update::decide(installed_version, release_json))
}

/// `nativeFinishMeeting(id, durationMs, note)`; `note` "" = none (F3-FR2, F2-FR3).
pub fn finish_meeting(id: &str, duration_ms: i64, note: &str) -> String {
    with_app(|app| app.finish_json(id, duration_ms, note))
}

/// `nativeSpend(fromMonth, toMonth)` (F16-FR5): spend per month, `YYYY-MM` inclusive.
pub fn spend(from: &str, to: &str) -> String {
    with_app(|app| app.spend_json(from, to))
}

/// `nativeRepriceAll(dryRun)` (F16-FR6): `{meetings, oldUsd, newUsd, …}`; writes only when
/// `dry_run` is false.
pub fn reprice_all(dry_run: bool) -> String {
    with_app(|app| app.reprice_json(dry_run))
}

/// `nativeGetRates()` (F16-FR4): `{asOf, entries, problems}`.
pub fn get_rates() -> String {
    with_app(App::get_rates_json)
}

/// `nativeSetRates(json)` (F16-FR4): the user's entries; replies as `nativeGetRates`.
pub fn set_rates(json: &str) -> String {
    with_app(|app| app.set_rates_json(json))
}

/// `nativeSearch(query, filter, limit)` (F19-FR2): the local index only.
pub fn search(query: &str, filter: &str, limit: i32) -> String {
    with_app(|app| app.search_json(query, filter, limit))
}

/// `nativeIndexStatus()` (F19-FR5): `{state, done, total}`.
pub fn index_status() -> String {
    with_app(App::index_status_json)
}

/// `nativeIndexUpdate()` (F19-FR5): re-index what changed. Blocking.
pub fn index_update() -> String {
    with_app(App::index_update_json)
}

/// `nativeRebuildIndex()` (F19-FR5): rebuild the index from the files. Blocking.
pub fn rebuild_index() -> String {
    with_app(App::rebuild_index_json)
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
    fn update_decide_needs_no_init_and_answers_junk_benignly() {
        assert_eq!(
            update_decide("0.1.0", "junk"),
            r#"{"updateAvailable":false}"#
        );
        let json = r#"{"tag_name":"v0.2.0","assets":[
            {"name":"meetings-v0.2.0.apk","browser_download_url":"https://example.test/a.apk"}]}"#;
        let d: serde_json::Value = serde_json::from_str(&update_decide("0.1.0-dev", json)).unwrap();
        assert_eq!(d["updateAvailable"], true);
        assert_eq!(d["version"], "0.2.0");
        assert_eq!(d["apkUrl"], "https://example.test/a.apk");
    }

    #[test]
    fn version_json_is_the_version() {
        assert_eq!(version_json(), crate::version());
        // release.yml stamps the tag's version into Cargo.toml (F14-FR2), so check its shape.
        let parts: Vec<&str> = crate::version().split(['.', '-']).collect();
        assert!(parts.len() >= 3, "{}", crate::version());
        assert!(parts[..3].iter().all(|p| p.parse::<u64>().is_ok()));
    }

    #[test]
    fn the_device_core_uses_the_real_hosts_over_https() {
        let c = Core::device("/data/x");
        // The literal, not the constant: a typo in DEFAULT_KEY_DIR must fail here.
        assert_eq!(
            c.key_dir,
            PathBuf::from("/storage/emulated/0/MyStyle/SnCopilot")
        );
        assert_eq!(c.endpoints, Endpoints::default());
        assert_eq!(c.timeout, providers::TEST_TIMEOUT);
        assert!(c.https_only);
    }
}
