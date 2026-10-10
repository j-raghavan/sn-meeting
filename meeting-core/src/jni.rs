//! JNI bridge (feature `jni-bridge`). Marshalling only: every body is `api::*` (host-tested).
//! `EnvUnowned::with_env` wraps the closure in `catch_unwind` (jni-0.22.4 env.rs:4793-4812);
//! `api::guarded` already turns core panics into `{"error":…}`, so this is the second net.
use jni::EnvUnowned;
use jni::objects::{JByteArray, JClass, JFloatArray, JString};
use jni::sys::{jboolean, jfloat, jint, jlong};

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeVersion<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::version_json())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInit<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    app_data_dir: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let dir: String = app_data_dir.try_to_string(env)?;
        env.new_string(crate::api::init(&dir))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking: Kotlin calls it on `CoreJobs`' background thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTestConnection<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    role: JString<'local>,
    provider: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let role: String = role.try_to_string(env)?;
        let provider: String = provider.try_to_string(env)?;
        env.new_string(crate::api::test_connection(&role, &provider))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeGetSettings<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::get_settings())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSetSettings<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let json: String = json.try_to_string(env)?;
        env.new_string(crate::api::set_settings(&json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeBeginMeeting<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    started_at_ms: jlong,
    mic_json: JString<'local>,
    audio_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let mic: String = mic_json.try_to_string(env)?;
        let audio: String = audio_json.try_to_string(env)?;
        env.new_string(crate::api::begin_meeting(started_at_ms, &mic, &audio))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeAddSegment<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    seg_path: JString<'local>,
    start_offset_ms: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let seg_path: String = seg_path.try_to_string(env)?;
        env.new_string(crate::api::add_segment(&id, &seg_path, start_offset_ms))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCloseSegment<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    seg_index: jint,
    duration_ms: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::close_segment(&id, seg_index, duration_ms))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeFinishMeeting<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    duration_ms: jlong,
    note: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let note: String = note.try_to_string(env)?;
        env.new_string(crate::api::finish_meeting(&id, duration_ms, &note))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeListRecent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    limit: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::list_recent(limit))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeGetMeeting<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::get_meeting(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeOpenTodos<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::open_todos())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSetTodoDone<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    todo_id: JString<'local>,
    done: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let todo_id: String = todo_id.try_to_string(env)?;
        env.new_string(crate::api::set_todo_done(&id, &todo_id, done))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRenameMeeting<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    title: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let title: String = title.try_to_string(env)?;
        env.new_string(crate::api::rename_meeting(&id, &title))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSetMeetingTemplate<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    template_id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let template_id: String = template_id.try_to_string(env)?;
        env.new_string(crate::api::set_meeting_template(&id, &template_id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeDeleteMeeting<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::delete_meeting(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRecover<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    service_alive: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::recover(service_alive))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking for minutes: Kotlin calls it on a job thread, never the UI thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTranscribe<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::transcribe(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking for minutes: "Transcribe again" on a job thread (F5-FR13).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTranscribeAgain<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::transcribe_again(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking: "Summarise again" on a job thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSummarise<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::summarise(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Returns at once; may be called from any thread while a job blocks another.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCancel<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::cancel(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeListTemplates<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::list_templates())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeGetTemplate<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::get_template(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSaveTemplate<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let json: String = json.try_to_string(env)?;
        env.new_string(crate::api::save_template(&json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeResetTemplate<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::reset_template(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativePreviewTemplate<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let json: String = json.try_to_string(env)?;
        env.new_string(crate::api::preview_template(&json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// F14-FR5: inkread's `nativeUpdateDecide` wrapper. The shell fetched `releases/latest`; the core
/// compares (semver) and returns the decision JSON. Junk in gives `{"updateAvailable":false}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeUpdateDecide<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    installed_version: JString<'local>,
    release_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let installed: String = installed_version.try_to_string(env)?;
        let json: String = release_json.try_to_string(env)?;
        env.new_string(crate::api::update_decide(&installed, &json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeGetRates<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::get_rates())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSetRates<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let json: String = json.try_to_string(env)?;
        env.new_string(crate::api::set_rates(&json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSpend<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    from_month: JString<'local>,
    to_month: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let from: String = from_month.try_to_string(env)?;
        let to: String = to_month.try_to_string(env)?;
        env.new_string(crate::api::spend(&from, &to))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Re-reads every listed sidecar; call it on `CoreJobs`' background thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRepriceAll<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    dry_run: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::reprice_all(dry_run))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeIndexStatus<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::index_status())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking (F19-FR5): call it on the index thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeIndexUpdate<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::index_update())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking (F19-FR5): call it on the index thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRebuildIndex<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::rebuild_index())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// The local index only (F19-FR10); call it on the search thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSearch<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    query: JString<'local>,
    filter: JString<'local>,
    limit: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let query: String = query.try_to_string(env)?;
        let filter: String = filter.try_to_string(env)?;
        env.new_string(crate::api::search(&query, &filter, limit))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking for up to 15 s (F19-FR9): call it on a background thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRecogniseQuery<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    png: JByteArray<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let bytes = env.convert_byte_array(&png)?;
        env.new_string(crate::api::recognise_query(&bytes))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// F18-FR13: never a full link URL.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCalendarLinks<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::calendar_links())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking (F18-FR4): one HTTPS GET per due link; call it on the calendar thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCalendarRefresh<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    force: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::calendar_refresh(force))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Reads the cache and the library; call it on `CoreJobs`' background thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeAgenda<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    from_day: JString<'local>,
    to_day: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let from: String = from_day.try_to_string(env)?;
        let to: String = to_day.try_to_string(env)?;
        env.new_string(crate::api::agenda(&from, &to))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeEvent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    event_key: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let key: String = event_key.try_to_string(env)?;
        env.new_string(crate::api::event(&key))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeBeginMeetingForEvent<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    started_at_ms: jlong,
    mic_json: JString<'local>,
    audio_json: JString<'local>,
    event_key: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let mic: String = mic_json.try_to_string(env)?;
        let audio: String = audio_json.try_to_string(env)?;
        let key: String = event_key.try_to_string(env)?;
        env.new_string(crate::api::begin_meeting_for_event(
            started_at_ms,
            &mic,
            &audio,
            &key,
        ))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeLinkEvent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    event_key: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let key: String = event_key.try_to_string(env)?;
        env.new_string(crate::api::link_event(&id, &key))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Pure (F15-FR4): called from the recorder's capture thread about twice a minute.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeChunkPlan<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    rms_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let rms: String = rms_json.try_to_string(env)?;
        env.new_string(crate::api::chunk_plan(&id, &rms))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeChunkReady<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    index: jint,
    path: JString<'local>,
    start_offset_ms: jlong,
    duration_ms: jlong,
    speech_ms: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let path: String = path.try_to_string(env)?;
        env.new_string(crate::api::chunk_ready(
            &id,
            index,
            &path,
            start_offset_ms,
            duration_ms,
            speech_ms,
        ))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeStartLive<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    send: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::start_live(&id, send))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking: Kotlin calls it on the near-live lane's thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeLiveDrain<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::live_drain(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeLiveTranscript<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    after_line: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::live_transcript(&id, after_line))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeChunksToCut<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::chunks_to_cut(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeAddMark<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    at_ms: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::add_mark(&id, at_ms))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeSetMarkLabel<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    mark_id: JString<'local>,
    label: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let mark_id: String = mark_id.try_to_string(env)?;
        let label: String = label.try_to_string(env)?;
        env.new_string(crate::api::set_mark_label(&id, &mark_id, &label))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeDeleteMark<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    mark_id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let mark_id: String = mark_id.try_to_string(env)?;
        env.new_string(crate::api::delete_mark(&id, &mark_id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// F15-FR1: pure; the capture thread calls the MP4 writer directly.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Open<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
    rate_hz: jint,
    channels: jint,
    asc: JByteArray<'local>,
    journal: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let path: String = path.try_to_string(env)?;
        let asc = env.convert_byte_array(&asc)?;
        let journal: String = journal.try_to_string(env)?;
        env.new_string(crate::api::mp4_open(
            &path, rate_hz, channels, &asc, &journal,
        ))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Write<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    frame: JByteArray<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let frame = env.convert_byte_array(&frame)?;
        env.new_string(crate::api::mp4_write(handle, &frame))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Close<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::mp4_close(handle))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Abandon<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::mp4_abandon(handle))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Cut<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    segment: JString<'local>,
    from_ms: jlong,
    to_ms: jlong,
    out: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let segment: String = segment.try_to_string(env)?;
        let out: String = out.try_to_string(env)?;
        env.new_string(crate::api::mp4_cut(&segment, from_ms, to_ms, &out))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeMp4Rebuild<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
    journal: JString<'local>,
    pieces_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let path: String = path.try_to_string(env)?;
        let journal: String = journal.try_to_string(env)?;
        let pieces: String = pieces_json.try_to_string(env)?;
        env.new_string(crate::api::mp4_rebuild(&path, &journal, &pieces))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// A Java float[] as a Vec (F17-FR4's batched points).
fn floats(env: &jni::Env, a: &JFloatArray) -> jni::errors::Result<Vec<f32>> {
    let mut v = vec![0f32; a.len(env)?];
    a.get_region(env, 0, &mut v)?;
    Ok(v)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeNotesOpen<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::notes_open(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkStroke<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    tool: jint,
    width: jfloat,
    points_xypt: JFloatArray<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        let points_xypt = floats(env, &points_xypt)?;
        env.new_string(crate::api::ink_stroke(
            &id,
            &layer,
            tool,
            width,
            &points_xypt,
        ))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkStrokeStyled<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    tool: jint,
    style_json: JString<'local>,
    points_xypt: JFloatArray<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let style_json: String = style_json.try_to_string(env)?;
        let points_xypt = floats(env, &points_xypt)?;
        env.new_string(crate::api::ink_stroke_styled(
            &id,
            tool,
            &style_json,
            &points_xypt,
        ))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkErase<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    points_xy: JFloatArray<'local>,
    r: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        let points_xy = floats(env, &points_xy)?;
        env.new_string(crate::api::ink_erase(&id, &layer, &points_xy, r))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkUndo<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        env.new_string(crate::api::ink_history(&id, &layer, false))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkRedo<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        env.new_string(crate::api::ink_history(&id, &layer, true))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkLasso<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    loop_xy: JFloatArray<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        let loop_xy = floats(env, &loop_xy)?;
        env.new_string(crate::api::ink_lasso(&id, &layer, &loop_xy))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkMove<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    ids_json: JString<'local>,
    dx: jfloat,
    dy: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        let ids_json: String = ids_json.try_to_string(env)?;
        env.new_string(crate::api::ink_move(&id, &layer, &ids_json, dx, dy))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkDelete<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    ids_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        let ids_json: String = ids_json.try_to_string(env)?;
        env.new_string(crate::api::ink_delete(&id, &layer, &ids_json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkStrokesForDraw<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    x0: jfloat,
    y0: jfloat,
    x1: jfloat,
    y1: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        env.new_string(crate::api::ink_strokes(&id, &layer, [x0, y0, x1, y1]))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCanvasForDraw<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    x0: jfloat,
    y0: jfloat,
    x1: jfloat,
    y1: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::canvas_for_draw(&id, [x0, y0, x1, y1]))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeCanvasEdit<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    op_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let op_json: String = op_json.try_to_string(env)?;
        env.new_string(crate::api::canvas_edit(&id, &op_json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkSave<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::ink_save(&id))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeNotesClose<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    cx: jfloat,
    cy: jfloat,
    zoom: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        env.new_string(crate::api::notes_close(&id, cx, cy, zoom))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeInkHit<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    layer: JString<'local>,
    x: jfloat,
    y: jfloat,
    r: jfloat,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let layer: String = layer.try_to_string(env)?;
        env.new_string(crate::api::ink_hit(&id, &layer, x, y, r))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeEditMinutes<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    patch_json: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let patch_json: String = patch_json.try_to_string(env)?;
        env.new_string(crate::api::edit_minutes(&id, &patch_json))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeAddTag<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    name: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let name: String = name.try_to_string(env)?;
        env.new_string(crate::api::add_tag(&id, &name))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRemoveTag<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: JString<'local>,
    name: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let id: String = id.try_to_string(env)?;
        let name: String = name.try_to_string(env)?;
        env.new_string(crate::api::remove_tag(&id, &name))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Rewrites sidecars: call it on the core thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeRenameTag<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    from: JString<'local>,
    to: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let from: String = from.try_to_string(env)?;
        let to: String = to.try_to_string(env)?;
        env.new_string(crate::api::rename_tag(&from, &to))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Reads every sidecar and may lay the graph out: call it on the core thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTagGraph<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    range: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let range: String = range.try_to_string(env)?;
        env.new_string(crate::api::tag_graph(&range))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Reads every sidecar: call it on the core thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTagMeetings<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
    range: JString<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let name: String = name.try_to_string(env)?;
        let range: String = range.try_to_string(env)?;
        env.new_string(crate::api::tag_meetings(&name, &range))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// Blocking, one provider call per meeting: call it on a background lane.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jraghavan_snmeeting_NativeBridge_nativeTagOlder<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    dry_run: jboolean,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        env.new_string(crate::api::tag_older(dry_run))
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
