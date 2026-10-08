//! JNI bridge (feature `jni-bridge`). Marshalling only: every body is `api::*` (host-tested).
//! `EnvUnowned::with_env` wraps the closure in `catch_unwind` (jni-0.22.4 env.rs:4793-4812);
//! `api::guarded` already turns core panics into `{"error":…}`, so this is the second net.
use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::jlong;

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
