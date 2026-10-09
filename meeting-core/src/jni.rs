//! JNI bridge (feature `jni-bridge`). Marshalling only: every body is `api::*` (host-tested).
//! `EnvUnowned::with_env` wraps the closure in `catch_unwind` (jni-0.22.4 env.rs:4793-4812);
//! `api::guarded` already turns core panics into `{"error":…}`, so this is the second net.
use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong};

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
