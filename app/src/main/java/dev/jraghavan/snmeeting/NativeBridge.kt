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

    // Settings (F10). The core owns settings.json.

    /** `{settings, origin, effective, keys}`: what is stored, what will run, each key's status. */
    external fun nativeGetSettings(): String

    /** An RFC 7396 merge patch; replies as [nativeGetSettings], or `{error, field}`. */
    external fun nativeSetSettings(json: String): String

    // Library (F3). The core serialises every sidecar write under one lock, so these may be
    // called from any background thread (CoreJobs in practice).

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

    // Providers (F5): long jobs, run on LongJobs' lane.

    /**
     * Transcribes the segments not yet done, then summarises. Blocking for minutes: call it on
     * a job thread, never the UI thread. Progress is in the sidecar (poll [nativeGetMeeting]).
     * `{meeting: <sidecar>}` once a job ran (its status says how it ended), or `{error}` when
     * nothing started ("meeting is busy", "Add copilot-key-openai.txt to MyStyle/SnCopilot").
     */
    external fun nativeTranscribe(id: String): String

    /** "Transcribe again" from transcribed or done: clears the segment texts (to-dos and titles
     *  stay), transcribes every segment, then summarises. Blocking, same replies as [nativeTranscribe]. */
    external fun nativeTranscribeAgain(id: String): String

    /** "Summarise again" on the stored transcript; blocking, same replies as [nativeTranscribe]. */
    external fun nativeSummarise(id: String): String

    /** Stops the meeting's running job: `{cancelled: bool}`. Returns at once, from any thread. */
    external fun nativeCancel(id: String): String

    // Templates (F11).

    /** Every template, built-ins first: `[{id, name, description, …, origin, builtin,
     *  customised, problem, path}]`; origin is `builtin`, `customised` or `yours`. */
    external fun nativeListTemplates(): String

    /** The effective version of one template, as a [nativeListTemplates] row. */
    external fun nativeGetTemplate(id: String): String

    /** Validates and writes the user copy; replies as [nativeGetTemplate], or
     *  `{error, line, field}` and nothing is written. Duplicate is a save under a new id. */
    external fun nativeSaveTemplate(json: String): String

    /** Deletes the user's file (a built-in goes back to the original): `{ok, origin}`. */
    external fun nativeResetTemplate(id: String): String

    /** Renders the layout against the sample meeting: `{markdown}` or `{error, line, field}`. */
    external fun nativePreviewTemplate(json: String): String

    // Spend (F16). Rates are plain numbers, never secrets.

    /** The rates in force: `{asOf, entries: [{provider, model, unit, perMinute?, input?, …,
     *  validFrom?, asOf, source, note?, origin}], problems: [line]}`; origin is `default` or `user`. */
    external fun nativeGetRates(): String

    /** The user's whole list of rate entries (`[]` resets to the defaults); replies as
     *  [nativeGetRates], or `{error, field}` and nothing is saved. Stored costs don't change. */
    external fun nativeSetRates(json: String): String

    // Updates (F14).

    /** Is `releaseJson` (GitHub releases/latest, fetched by the shell) newer than
     *  `installedVersion`? `{updateAvailable, version, notes, apkUrl, sha256Url}`; junk gives
     *  `{"updateAvailable":false}`. Needs no [nativeInit]. */
    external fun nativeUpdateDecide(installedVersion: String, releaseJson: String): String
}
