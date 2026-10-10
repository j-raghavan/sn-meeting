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

/// `nativeChunkReady(id, index, path, startOffsetMs, durationMs, speechMs)` (F15-FR5): a chunk
/// file is finalised; `speechMs` is -1 when unknown. `{index, state, added}`.
pub fn chunk_ready(
    id: &str,
    index: i32,
    path: &str,
    start_offset_ms: i64,
    duration_ms: i64,
    speech_ms: i64,
) -> String {
    with_app(|app| {
        let c = crate::library::ChunkIn {
            index,
            path,
            start_offset_ms,
            duration_ms,
            speech_ms,
        };
        app.chunk_ready_json(id, c)
    })
}

/// `nativeStartLive(id, send)` (F15-FR5 / FR12): the near-live recorder began `id`.
pub fn start_live(id: &str, send: bool) -> String {
    with_app(|app| app.start_live_json(id, send))
}

/// `nativeLiveDrain(id)` (F15-FR5): send the pending chunks. Blocking.
pub fn live_drain(id: &str) -> String {
    with_app(|app| app.live_drain_json(id))
}

/// `nativeLiveTranscript(id, afterLine)` (F15-FR6): the live transcript's lines and status.
pub fn live_transcript(id: &str, after_line: i32) -> String {
    with_app(|app| app.live_transcript_json(id, after_line))
}

/// `nativeChunksToCut(id)` (F15-FR9 / FR10): the chunk files to re-cut from their segments.
pub fn chunks_to_cut(id: &str) -> String {
    with_app(|app| app.chunks_to_cut_json(id))
}

/// `nativeAddMark(id, atMs)` (F15-FR11): `{markId}`.
pub fn add_mark(id: &str, at_ms: i64) -> String {
    with_app(|app| app.add_mark_json(id, at_ms))
}

/// `nativeSetMarkLabel(id, markId, label)` (F15-FR11); "" clears it.
pub fn set_mark_label(id: &str, mark_id: &str, label: &str) -> String {
    with_app(|app| app.set_mark_label_json(id, mark_id, label))
}

/// `nativeDeleteMark(id, markId)` (F15-FR11).
pub fn delete_mark(id: &str, mark_id: &str) -> String {
    with_app(|app| app.delete_mark_json(id, mark_id))
}

// Notes (F17). Ink calls are short: they run on the CoreJobs thread.

/// `nativeNotesOpen(id)` (F17-FR5): `{view, clock, bounds, strokes}`.
pub fn notes_open(id: &str) -> String {
    with_app(|app| app.notes_open_json(id))
}

/// `nativeInkStroke(id, layer, tool, width, xypt)` (F17-FR4): one batched stroke.
pub fn ink_stroke(id: &str, layer: &str, tool: i32, width: f32, xypt: &[f32]) -> String {
    with_app(|app| app.ink_stroke_json(id, layer, tool, width, xypt))
}

/// `nativeInkErase(id, layer, xy, r)`: an eraser sweep, one undo step.
pub fn ink_erase(id: &str, layer: &str, xy: &[f32], r: f32) -> String {
    with_app(|app| app.ink_erase_json(id, layer, xy, r))
}

/// `nativeInkUndo` / `nativeInkRedo` (F13-FR5).
pub fn ink_history(id: &str, layer: &str, redo: bool) -> String {
    with_app(|app| app.ink_history_json(id, layer, redo))
}

/// `nativeInkLasso(id, layer, loopXy)`: the strokes a loop takes.
pub fn ink_lasso(id: &str, layer: &str, xy: &[f32]) -> String {
    with_app(|app| app.ink_lasso_json(id, layer, xy))
}

/// `nativeInkMove(id, layer, idsJson, dx, dy)`: moves a lasso selection.
pub fn ink_move(id: &str, layer: &str, ids: &str, dx: f32, dy: f32) -> String {
    with_app(|app| app.ink_move_json(id, layer, ids, dx, dy))
}

/// `nativeInkDelete(id, layer, idsJson)`: deletes a lasso selection.
pub fn ink_delete(id: &str, layer: &str, ids: &str) -> String {
    with_app(|app| app.ink_delete_json(id, layer, ids))
}

/// `nativeInkStrokesForDraw(id, layer, x0, y0, x1, y1)`: the strokes in a world rectangle.
pub fn ink_strokes(id: &str, layer: &str, rect: [f32; 4]) -> String {
    with_app(|app| app.ink_strokes_json(id, layer, rect))
}

/// `nativeCanvasForDraw(id, x0, y0, x1, y1)` (F21-FR1): the canvas's strokes and items in a
/// world rectangle, in paint order.
pub fn canvas_for_draw(id: &str, rect: [f32; 4]) -> String {
    with_app(|app| app.canvas_for_draw_json(id, rect))
}

/// `nativeCanvasEdit(id, opJson)` (F21): one gesture on the canvas, one undo step.
pub fn canvas_edit(id: &str, op: &str) -> String {
    with_app(|app| app.canvas_edit_json(id, op))
}

/// `nativeInkHit(id, layer, x, y, r)` (F17-FR7): Listen's tap on ink.
pub fn ink_hit(id: &str, layer: &str, x: f32, y: f32, r: f32) -> String {
    with_app(|app| app.ink_hit_json(id, layer, x, y, r))
}

/// `nativeEditMinutes(id, patchJson)` (F17-FR10): a correction, marked the user's.
pub fn edit_minutes(id: &str, patch: &str) -> String {
    with_app(|app| app.edit_minutes_json(id, patch))
}

/// `nativeInkSave(id)` (F17-FR5): the deferred flush.
pub fn ink_save(id: &str) -> String {
    with_app(|app| app.ink_save_json(id))
}

/// `nativeNotesClose(id, cx, cy, zoom)`: saves the ink and the viewport, lets the layers go.
pub fn notes_close(id: &str, cx: f32, cy: f32, zoom: f32) -> String {
    with_app(|app| app.notes_close_json(id, f64::from(cx), f64::from(cy), f64::from(zoom)))
}

/// F15-FR1 (D14 revised 2026-10-09): the near-live recorder's `.m4a` files, written by the
/// core because `MediaMuxer.stop()` aborts the process on the Manta's firmware. Pure: no
/// `nativeInit` needed; the capture thread calls these directly.
pub fn mp4_open(path: &str, rate_hz: i32, channels: i32, asc: &[u8], journal: &str) -> String {
    guarded(|| crate::mp4::open_json(path, rate_hz, channels, asc, journal))
}

/// One AAC access unit; "" when written.
pub fn mp4_write(handle: i64, frame: &[u8]) -> String {
    guarded(|| crate::mp4::write_json(handle, frame))
}

/// Finalise: `{frames, durationMs}`.
pub fn mp4_close(handle: i64) -> String {
    guarded(|| crate::mp4::close_json(handle))
}

/// Let a file go without its index (its journal stays for recovery).
pub fn mp4_abandon(handle: i64) -> String {
    guarded(|| crate::mp4::abandon_json(handle))
}

/// A chunk re-cut from a segment (F15-FR9/FR10): `{frames}`.
pub fn mp4_cut(segment: &str, from_ms: i64, to_ms: i64, out: &str) -> String {
    guarded(|| crate::mp4::cut_json(segment, from_ms, to_ms, out))
}

/// A segment repaired from its journal or rebuilt from its chunks (F15-FR8).
pub fn mp4_rebuild(path: &str, journal: &str, pieces_json: &str) -> String {
    guarded(|| crate::mp4::rebuild_json(path, journal, pieces_json))
}

/// `nativeChunkPlan(id, rmsJson)` (F15-FR4): where the chunk being written ends,
/// `{cutAtMs|null, overlapMs, speechMs}`. Pure: it needs no `nativeInit`, so the recorder's
/// capture thread can ask at any time.
pub fn chunk_plan(id: &str, rms_json: &str) -> String {
    guarded(|| crate::chunk::plan_json(id, rms_json))
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

/// `nativeRecogniseQuery(png)` (F19-FR9): blocking, at most 15 s.
pub fn recognise_query(png: &[u8]) -> String {
    with_app(|app| app.recognise_query_json(png))
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

/// `nativeCalendarLinks()` (F18-FR13): each link's id, name and elided URL, never the URL.
pub fn calendar_links() -> String {
    with_app(App::calendar_links_json)
}

/// `nativeCalendarRefresh(force)` (F18-FR4): fetch the due links. Blocking.
pub fn calendar_refresh(force: bool) -> String {
    with_app(|app| app.calendar_refresh_json(force))
}

/// `nativeAgenda(fromDay, toDay)` (F18-FR7): the days Home shows, from the cache and the
/// library.
pub fn agenda(from: &str, to: &str) -> String {
    with_app(|app| app.agenda_json(from, to))
}

/// `nativeEvent(eventKey)` (F18-FR8): the event sheet.
pub fn event(key: &str) -> String {
    with_app(|app| app.event_json(key))
}

/// `nativeLinkEvent(id, eventKey)` (F18-FR10); `""` unlinks.
pub fn link_event(id: &str, key: &str) -> String {
    with_app(|app| app.link_event_json(id, key))
}

/// `nativeBeginMeetingForEvent(startedAtMs, micJson, audioJson, eventKey)` (F18-FR8): as
/// `nativeBeginMeeting`, linked to the event (`linkSource: user`).
pub fn begin_meeting_for_event(
    started_at_ms: i64,
    mic_json: &str,
    audio_json: &str,
    key: &str,
) -> String {
    with_app(|app| {
        app.begin_linked(
            started_at_ms,
            mic_json,
            audio_json,
            Some(key).filter(|k| !k.is_empty()),
        )
    })
}

/// `nativeAddTag(id, name)` (F20-FR4): `{tags}`.
pub fn add_tag(id: &str, name: &str) -> String {
    with_app(|app| app.add_tag_json(id, name))
}

/// `nativeRemoveTag(id, name)` (F20-FR4): `{tags}`.
pub fn remove_tag(id: &str, name: &str) -> String {
    with_app(|app| app.remove_tag_json(id, name))
}

/// `nativeRenameTag(from, to)` (F20-FR4): every meeting, resumable. `{renamed, to}`.
pub fn rename_tag(from: &str, to: &str) -> String {
    with_app(|app| app.rename_tag_json(from, to))
}

/// `nativeTagGraph(range)` (F20-FR7 / FR9): the laid-out Tag Graph, `all` or `3m`.
pub fn tag_graph(range: &str) -> String {
    with_app(|app| app.tag_graph_json(range))
}

/// `nativeTagMeetings(name, range)` (F20-FR8): the tag's recordings, newest first.
pub fn tag_meetings(name: &str, range: &str) -> String {
    with_app(|app| app.tag_meetings_json(name, range))
}

/// `nativeTagOlder(dryRun)` (F20-FR10): count and price first, then one tags-only call per untagged meeting.
pub fn tag_older(dry_run: bool) -> String {
    with_app(|app| app.tag_older_json(dry_run))
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
    fn chunk_plan_needs_no_init() {
        let r = chunk_plan("meeting-20261009-100000", r#"{"rms":[],"limitMs":0}"#);
        assert_eq!(r, r#"{"cutAtMs":0,"overlapMs":0,"speechMs":0}"#);
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
