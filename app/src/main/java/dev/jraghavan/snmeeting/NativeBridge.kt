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

    // Library (F3). The core serialises every sidecar write under one lock, so these may be
    // called from any background thread (CoreJobs in practice). Not wired into the UI until M7.

    /** Starts a meeting: `{id, firstSegmentPath, plan: {maxDurationMs, maxFileBytes}}`. */
    external fun nativeBeginMeeting(startedAtMs: Long, micJson: String, audioJson: String): String

    /** The recorder rolled over: `segPath` must be the next `<id>-NN.m4a`. `{index, path}`. */
    external fun nativeAddSegment(id: String, segPath: String, startOffsetMs: Long): String

    /** A finalised segment's measured duration; `segIndex` is 0-based. `{ok: true}`. */
    external fun nativeCloseSegment(id: String, segIndex: Int, durationMs: Long): String

    /** Recording stopped; `note` is why ("mic disconnected"), "" for none. `{ok: true}`. */
    external fun nativeFinishMeeting(id: String, durationMs: Long, note: String): String

    /** The `limit` newest meetings across the current and previous save folders. */
    external fun nativeListRecent(limit: Int): String

    /** `{meeting: <sidecar>, source, dir, segmentPaths, transcriptPath}`. */
    external fun nativeGetMeeting(id: String): String

    /** Open to-dos from every meeting, newest meeting first: `[TodoRef]`. */
    external fun nativeOpenTodos(): String

    /** Ticks or unticks one to-do. `{ok: true}`. */
    external fun nativeSetTodoDone(id: String, todoId: String, done: Boolean): String

    /** Names the meeting (`titleSource = user`); "" returns it to untitled. `{ok: true}`. */
    external fun nativeRenameMeeting(id: String, title: String): String

    /** The template the next Transcribe or Summarise again uses. `{ok: true}`. */
    external fun nativeSetMeetingTemplate(id: String, templateId: String): String

    /** Removes the meeting's audio, sidecar and transcript; refused while recording or a job
     *  runs. `{ok: true, removed: [file names]}`. */
    external fun nativeDeleteMeeting(id: String): String

    /** Crash recovery on launch and resume: `{recovered: [id], measure: [{id, index, path}],
     *  flagged: [{id, index, flag}]}`. Measure each listed file and close it. */
    external fun nativeRecover(serviceAlive: Boolean): String
}
