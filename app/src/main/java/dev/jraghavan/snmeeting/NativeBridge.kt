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

    // Notes (F17). World coordinates (screen px at 100 %); times on the recording clock. A
    // `layer` is "canvas" (the Notes tab) or "minutes" (the Minutes tab's ink area, F17-FR10).
    // Short calls: run them on CoreJobs.

    /** Opens [id]'s canvas: `{view: {cx, cy, zoom}|null, clock: "recording-ms", bounds:
     *  [x0, y0, x1, y1]|null, strokes, items}`. A damaged canvas file is an `{error}`, never saved over. */
    external fun nativeNotesOpen(id: String): String

    /** F17-FR4: one whole stroke, `[x, y, pressure, t]` per point, `t` recording-clock ms or -1
     *  (untimed); [tool] 0 pen, 1 highlighter. The first stroke names the notes in the sidecar.
     *  `{strokeId, bbox, atMs}`. Kept in memory until [nativeInkSave]. */
    external fun nativeInkStroke(id: String, layer: String, tool: Int, width: Float, pointsXypt: FloatArray): String

    /** F21-FR7: a canvas stroke with a style (`{color, opacity, size, …}`): the Pen's true colour
     *  goes in `.inkbin`, its width is the size's, its style ids go in `items.json`; [tool] 0
     *  pen, 1 highlighter (unchanged). `{strokeId, bbox, atMs}`. */
    external fun nativeInkStrokeStyled(id: String, tool: Int, styleJson: String, pointsXypt: FloatArray): String

    /** An eraser sweep through `[x, y, …]` with radius [r], one undo step. `{erased: [id], bbox}`. */
    external fun nativeInkErase(id: String, layer: String, pointsXy: FloatArray, r: Float): String

    /** F13-FR5: `{changed, bbox, canUndo, canRedo}`. */
    external fun nativeInkUndo(id: String, layer: String): String

    /** F13-FR5: `{changed, bbox, canUndo, canRedo}`. */
    external fun nativeInkRedo(id: String, layer: String): String

    /** The strokes a lasso loop `[x, y, …]` takes (enclosed or crossed): `{ids, bbox}`. */
    external fun nativeInkLasso(id: String, layer: String, loopXy: FloatArray): String

    /** Moves the strokes `[id, …]` by ([dx], [dy]), one undo step: `{moved, bbox}`. */
    external fun nativeInkMove(id: String, layer: String, idsJson: String, dx: Float, dy: Float): String

    /** Deletes the strokes `[id, …]`, one undo step: `{deleted: [id], bbox}`. */
    external fun nativeInkDelete(id: String, layer: String, idsJson: String): String

    /** The strokes meeting a world rectangle, in paint order: `{strokes: [{id, tool, width,
     *  color: [r, g, b, a], atMs, pts: [x, y, …]}], bounds, canUndo, canRedo}`. */
    external fun nativeInkStrokesForDraw(id: String, layer: String, x0: Float, y0: Float, x1: Float, y1: Float): String

    // Notes tools as sn-canvas (F21, ADR D21). The canvas is a scene: strokes and items with
    // one undo history, owned by the core; `ref` is "s:<strokeId>" or "i:<itemId>".

    /** F21-FR1: the canvas's strokes and items meeting a world rectangle, in paint order, bound
     *  arrow ends resolved: `{entries: [{ref, stroke: {…}} | {ref, item: {…}}], bounds,
     *  canUndo, canRedo, selection: [ref], selectionBounds, itemsOk}`. */
    external fun nativeCanvasForDraw(id: String, x0: Float, y0: Float, x1: Float, y1: Float): String

    /** F21: one gesture on the canvas (`{op, …}`: `clear`, `restoreCleared`, …), one undo step:
     *  `{dirty: [x0, y0, x1, y1]|null, selection: [ref], canUndo, canRedo}` or `{error}`. */
    external fun nativeCanvasEdit(id: String, opJson: String): String

    /** F21-FR10/FR15: the topmost stroke or item within [r] of a world point: `{ref|null, cell:
     *  {row, col}|null, atMs|null}`, and for a timed one where Listen starts (`seekMs`, `segIndex`,
     *  `segOffsetMs`, `lineN`). */
    external fun nativeCanvasHit(id: String, x: Float, y: Float, r: Float): String

    /** F21-FR11: insert the image at [srcPath] ([pxW] × [pxH] from its header): the core checks
     *  it (extension against first bytes, ≤ 20 MB, ≤ 50 MP, no links), copies it into the notes
     *  folder's `images/` and places it in the view (`{rect: [x0, y0, x1, y1], recMs}`), one undo
     *  step. `{itemId, dirty, selection, canUndo, canRedo}` or `{error}`. */
    external fun nativeCanvasImportImage(id: String, srcPath: String, pxW: Int, pxH: Int, viewJson: String): String

    /** F17-FR7 (Listen): the topmost stroke within [r] of a world point. Timed: `{strokeId, atMs,
     *  seekMs, segIndex, segOffsetMs, lineN}` (seekMs = max(0, t − 2 s)); untimed `{strokeId,
     *  atMs: null}`; none `{strokeId: null}`. */
    external fun nativeInkHit(id: String, layer: String, x: Float, y: Float, r: Float): String

    /** F17-FR10: one correction to the minutes — `{"summary": text}`, `{"decision": {index,
     *  text}}` (index = count adds; "" deletes) or `{"section": {index, body}}`; the field
     *  becomes the user's and Summarise again keeps it. `{meeting}`. */
    external fun nativeEditMinutes(id: String, patchJson: String): String

    /** F20-FR4: add the user's tag (normalised by the core) to meeting [id]. `{tags: [{name,
     *  source}]}`, or `{error}`. */
    external fun nativeAddTag(id: String, name: String): String

    /** F20-FR4: remove a tag; a later summary won't add it back. `{tags}`. */
    external fun nativeRemoveTag(id: String, name: String): String

    /** F20-FR4: rename a tag in every meeting in the save folders, merging into [to] where it
     *  exists; resumable. Rewrites sidecars: call it on the core thread. `{renamed, to}`. */
    external fun nativeRenameTag(from: String, to: String): String

    /** F20-FR7 / FR9: the Tag Graph for [range] (`all` or `3m`), laid out by the core once per
     *  set of tags and counts: `{range, graph: {nodes: [{name, count, x, y, r}], edges: [{a, b,
     *  shared, weak}], meetings, bounds: [x0, y0, x1, y1]}, laidOut}`. Call it on the core thread. */
    external fun nativeTagGraph(range: String): String

    /** F20-FR8: the recordings tagged [name] in [range], newest first: `{tag, count, meetings:
     *  [{id, title, startedAt, startedAtMs, durationMs, tags}]}` (`tags`: its other tags). */
    external fun nativeTagMeetings(name: String, range: String): String

    /** F20-FR10 / OQ31: "Tag older meetings…". [dryRun]: `{meetings, estUsd|null, provider,
     *  model}` to show first; else one tags-only call per untagged meeting, `{tagged, failed,
     *  error}`. Blocking for minutes: call it on [LongJobs]' lane. */
    external fun nativeTagOlder(dryRun: Boolean): String

    /** F17-FR5's deferred flush (on a 1.5 s timer after a change): `{saved}`. */
    external fun nativeInkSave(id: String): String

    /** The page goes: the ink is saved, the viewport kept (centre and zoom), the layers let go. */
    external fun nativeNotesClose(id: String, cx: Float, cy: Float, zoom: Float): String

    // Near-live audio files (F15-FR1, D14 revised 2026-10-09). MediaMuxer.stop() aborts the
    // process on the Manta's firmware, so the core writes the .m4a files. Pure: no nativeInit.

    /** A new `.m4a` for AAC-LC frames: [asc] is the encoder's csd-0; [journal] ("" for none)
     *  keeps each frame's size so a crashed file can be repaired. `{handle}` or `{error}`. */
    external fun nativeMp4Open(path: String, rateHz: Int, channels: Int, asc: ByteArray, journal: String): String

    /** One access unit, in order: "" when written, else `{error}`. */
    external fun nativeMp4Write(handle: Long, frame: ByteArray): String

    /** Writes the index and closes: `{frames, durationMs}`; a file with no frame is removed. */
    external fun nativeMp4Close(handle: Long): String

    /** Lets a failed file go without its index; its journal stays for recovery. `{ok}`. */
    external fun nativeMp4Abandon(handle: Long): String

    /** F15-FR9/FR10: chunk [out] re-cut from [segment]'s frames in [fromMs, toMs): `{frames}`. */
    external fun nativeMp4Cut(segment: String, fromMs: Long, toMs: Long, out: String): String

    /** F15-FR8: segment [path] repaired from [journal], else rebuilt from its chunks
     *  (`[{path, skipMs}]`): `{frames, from}` or `{error}`. */
    external fun nativeMp4Rebuild(path: String, journal: String, piecesJson: String): String

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

    /** F15-FR9 / FR10: the chunk files the next transcription needs that are gone, each to be
     *  re-cut from its segment (`[{index, segIndex, fromMs, toMs, segPath, path, startOffsetMs,
     *  durationMs}]`), then reported with [nativeChunkReady]. */
    external fun nativeChunksToCut(id: String): String

    /** F15-FR11: a mark at recording-clock [atMs] (LiveClock). `{markId}`. */
    external fun nativeAddMark(id: String, atMs: Long): String

    /** F15-FR11: names a mark; "" clears the label. `{ok: true}`. */
    external fun nativeSetMarkLabel(id: String, markId: String, label: String): String

    /** F15-FR11: `{ok: true}`. */
    external fun nativeDeleteMark(id: String, markId: String): String

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
