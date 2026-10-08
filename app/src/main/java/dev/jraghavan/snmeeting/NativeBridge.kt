package dev.jraghavan.snmeeting

/**
 * The Rust core, libmeeting.so (spec Interfaces). Strings in, JSON out; a core failure comes
 * back as `{"error": "..."}`, never as a thrown panic. No function takes or returns an API key:
 * the core reads key files itself (F4-FR4).
 */
object NativeBridge {
    init {
        System.loadLibrary("meeting")
    }

    /** The core's crate version (F1-FR5). */
    external fun nativeVersion(): String

    /** Creates the core's runtime and records the app data dir. Idempotent; cheap. */
    external fun nativeInit(appDataDir: String): String

    /**
     * Lists models with the provider's key (no audio, no cost; F10-FR5). Blocking for up to
     * 15 s, so call it through [CoreJobs]. `role` is `transcribe` or `summarise`.
     */
    external fun nativeTestConnection(role: String, provider: String): String

    // Settings (F10). The core owns settings.json; these are not wired into the UI until M7.

    /** `{settings, origin, effective, keys}`: what is stored, what will run, each key's status. */
    external fun nativeGetSettings(): String

    /** An RFC 7396 merge patch; replies as [nativeGetSettings], or `{error, field}`. */
    external fun nativeSetSettings(json: String): String
}
