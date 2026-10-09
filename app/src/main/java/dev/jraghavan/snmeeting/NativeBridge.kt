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

    // Near-live transcript (F15).

    /** F15-FR4: where the chunk being written ends, from its RMS per 100 ms block
     *  (`{"rms": [..], "limitMs": n|null}`): `{cutAtMs|null, overlapMs, speechMs}`. Pure and
     *  quick; it needs no [nativeInit], so the recorder's capture thread calls it directly. */
    external fun nativeChunkPlan(id: String, rmsJson: String): String

    /** F15-FR5: chunk [index]'s file is finalised at [path] (`<filesDir>/chunks/<id>/c-NNNN.m4a`);
     *  [speechMs] is from [nativeChunkPlan], -1 when unknown. While recording it is recorded
     *  `pending`; afterwards only a known chunk's re-cut file. `{index, state, added}`. */
    external fun nativeChunkReady(id: String, index: Int, path: String, startOffsetMs: Long, durationMs: Long, speechMs: Long): String

    /** F15-FR5 / FR12: the near-live recorder began [id]; the transcribe provider is fixed now.
     *  [send] false (the notice declined) keeps the chunks waiting for Transcribe. */
    external fun nativeStartLive(id: String, send: Boolean): String

    /** F15-FR5: sends [id]'s pending chunks in order while it records. Blocking (a request is
     *  up to ~70 s): call it on [LiveJobs]' thread only. `{state, done, waiting, retryInMs?,
     *  reason?}`; state is idle, retry, paused, blocked, stopped, off or busy. */
    external fun nativeLiveDrain(id: String): String

    /** F15-FR6: the live transcript's whole lines after line [afterLine] (0: all), each with its
     *  estimated recording-clock time, and its status: `{lines: [{n, atMs, text}], waiting,
     *  state: off|paused|offline|transcribing|current, reason?, spentUsd, spent}`. */
    external fun nativeLiveTranscript(id: String, afterLine: Int): String

    // Spend (F16). Rates are plain numbers, never secrets.

    /** Spend per local month, `YYYY-MM` inclusive, over every listed sidecar: `{months: [{month,
     *  usd, meetings, unpriced, label, byProvider: {openai: {transcribe, summarise}}, search, searches}],
     *  unpriced}`; `search` is handwritten queries (F19-FR9), included in `usd`.
     *  `label` is Home's line ("October · $3.20 spent · 6 meetings"). */
    external fun nativeSpend(fromMonth: String, toMonth: String): String

    /** "Re-price with current rates": every stored record priced again from its counters.
     *  `{meetings, written, oldUsd, newUsd, unpricedBefore, unpricedAfter}`; `meetings` is how
     *  many change. Writes only when `dryRun` is false, so ask with true first and confirm. */
    external fun nativeRepriceAll(dryRun: Boolean): String

    /** The rates in force: `{asOf, entries: [{provider, model, unit, perMinute?, input?, …,
     *  validFrom?, asOf, source, note?, origin}], problems: [line]}`; origin is `default` or `user`. */
    external fun nativeGetRates(): String

    /** The user's whole list of rate entries (`[]` resets to the defaults); replies as
     *  [nativeGetRates], or `{error, field}` and nothing is saved. Stored costs don't change. */
    external fun nativeSetRates(json: String): String

    // Search (F19). The index is a cache in app-private storage, rebuilt from the files.

    /** Searches the local index only (never a provider, F19-FR10). `filter` is all, notes,
     *  transcript, minutes, todos or events; `limit` the most groups (0 = 50). `{query, filter,
     *  groups: [{kind, id, title, start, startMs, hitCount, hits: [{kind, label, unitId, atMs,
     *  time, snippet: [{text, bold}], region, bbox, strokeIds, changed}]}], more, totalGroups,
     *  indexing}`. */
    external fun nativeSearch(query: String, filter: String, limit: Int): String

    /** Reads a handwritten query strip (a PNG, at most 1568 × 200 px) with the summarise
     *  provider, the only Search call that leaves the device (F19-FR9). Blocking for up to 15 s.
     *  `{text}`, or `{error, offline}`: fall back to the keyboard either way. */
    external fun nativeRecogniseQuery(png: ByteArray): String

    /** `{state: ready|indexing|missing, done, total}`; "Indexing 120/340" while it runs. */
    external fun nativeIndexStatus(): String

    /** Re-indexes the meetings and calendar links whose files changed; blocking, so call it on
     *  a background thread, never the UI thread. `{indexed, removed, rebuilt}`, or `{busy: true}`. */
    external fun nativeIndexUpdate(): String

    /** Settings → "Rebuild search index": starts the index again from the files. Blocking,
     *  same replies as [nativeIndexUpdate]. */
    external fun nativeRebuildIndex(): String

    // Calendar (F18). A link's URL never crosses: links are named by id, calendar name and an
    // elided form ("https://calendar.google.com/….ics").

    /** `[{linkId, name, elided, label, lastOk, lastAttempt, status, reason, eventsThisWeek,
     *  skipped}]`; status is never, ok, gone ("This calendar link no longer works") or failed. */
    external fun nativeCalendarLinks(): String

    /** Fetches the links that are due (all of them when [force]); blocking, one HTTPS GET of at
     *  most 30 s per link, so call it on [CalendarJobs]. `{updatedAtMs, failed: [linkId],
     *  fetched, links, deferred?}` or `{busy: true}`. */
    external fun nativeCalendarRefresh(force: Boolean): String

    /** Home's days, `YYYY-MM-DD` inclusive (at most 6 weeks): `{hasLinks, today, nowMs,
     *  updatedAtMs, failed, days: [{day, dot, allDay: [row], rows: [row]}]}`. A row is an event
     *  (`kind: event`, eventKey, title, startMs, endMs, location, people, calendar, recurring,
     *  current, meetingId, marks, fromLastTime when current) or a meeting (`kind: meeting`,
     *  meetingId, title, unscheduled, marks); marks are `{recording, notes, minutes, todosOpen,
     *  todosTotal}`. Read from the cache: no network. */
    external fun nativeAgenda(fromDay: String, toDay: String): String

    /** An event's sheet: `{calendar, event: {eventKey, title, startMs, endMs, allDay, location,
     *  attendees, agenda, recurring, tzUnknown, …}, template: {id, keyword}, series: [{meetingId,
     *  title, startedAt, openTodos}], fromLastTime: [TodoRef], linked: [meetingId]}`, or
     *  `{error}` once the event has left the calendar. */
    external fun nativeEvent(eventKey: String): String

    /** "Link to event…" (`linkSource: user`, never replaced by the automatic rule), or "" to
     *  unlink: the meeting is unscheduled. `{ok: true}`, or `{error}`. */
    external fun nativeLinkEvent(id: String, eventKey: String): String

    /** As [nativeBeginMeeting], linked to the event (`linkSource: user`) when it is still in
     *  the calendar; recording never depends on that (an unknown key records unlinked). */
    external fun nativeBeginMeetingForEvent(startedAtMs: Long, micJson: String, audioJson: String, eventKey: String): String

    // Updates (F14).

    /** Is `releaseJson` (GitHub releases/latest, fetched by the shell) newer than
     *  `installedVersion`? `{updateAvailable, version, notes, apkUrl, sha256Url}`; junk gives
     *  `{"updateAvailable":false}`. Needs no [nativeInit]. */
    external fun nativeUpdateDecide(installedVersion: String, releaseJson: String): String
}
