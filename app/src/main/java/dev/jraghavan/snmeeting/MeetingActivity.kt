package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.Actions
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.Format
import dev.jraghavan.snmeeting.model.LiveTranscript
import dev.jraghavan.snmeeting.model.MeetingHeader
import dev.jraghavan.snmeeting.model.MeetingView
import dev.jraghavan.snmeeting.model.ProviderNotice
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.Sidecar
import dev.jraghavan.snmeeting.model.Status
import dev.jraghavan.snmeeting.model.SummaryCard
import dev.jraghavan.snmeeting.model.TemplateChoice
import dev.jraghavan.snmeeting.model.TemplateRow
import dev.jraghavan.snmeeting.model.Todo
import dev.jraghavan.snmeeting.model.TranscriptPages
import dev.jraghavan.snmeeting.play.MediaPlayerPort
import dev.jraghavan.snmeeting.play.Playback
import dev.jraghavan.snmeeting.play.SegmentPlaylist
import dev.jraghavan.snmeeting.record.deviceRecoverer
import dev.jraghavan.snmeeting.record.RecState
import dev.jraghavan.snmeeting.record.RecordingService

/**
 * One meeting (spec F7) in the Inkwell style (F9), read from the core: the title (tap to
 * rename), when and how long, playback, the template, the one action its status allows, its
 * to-dos and the transcript paged for e-ink.
 */
class MeetingActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    private lateinit var id: String
    private var view: MeetingView? = null
    private var page: ScrollView? = null
    private var settings: SettingsView? = null
    private var templates: List<TemplateRow> = emptyList()
    private var pickingTemplate = false
    private var actions: Actions? = null
    private lateinit var cards: MeetingCards
    private var transcriptPage = 0
    private var progress: TextView? = null
    /** F19-FR3: a search hit's moment to cue playback to, and the part to scroll to, once. */
    private var cueAt: Long? = null
    private var focus: String? = null
    private var mark: Pair<Int, Int>? = null
    private val anchors = HashMap<String, View>()

    // F15-FR6 / F17-FR6: the live transcript while a near-live meeting records. Only its own
    // views change, and only when a reply differs (new lines, a new status, the cost).
    private var live: LiveTranscript? = null
    private var liveJson: String? = null
    private var liveAll = false
    private var liveLines: LinearLayout? = null
    private var liveStatus: TextView? = null
    private var unLive: (() -> Unit)? = null
    private var unHub: (() -> Unit)? = null
    private val livePoll = object : Runnable {
        override fun run() {
            fetchLive()
            handler.postDelayed(this, LIVE_POLL_MS)
        }
    }

    /** F7-FR2: while this meeting's job runs, the sidecar is re-read once a second for progress. */
    private val poll = object : Runnable {
        override fun run() {
            if (!LongJobs.lane.isRunning(id)) return
            scope.core({ NativeBridge.nativeGetMeeting(id) }) { json ->
                val v = CoreJson.obj(json, MeetingView::from).getOrNull() ?: return@core
                val next = actionsFor(v.meeting)
                // Only the label moves: change it in place (no page redraw). Anything else: rebuild.
                if (next.copy(progress = null) == actions?.copy(progress = null)) {
                    view = v; actions = next; progress?.text = next.progress
                } else {
                    view = v; render()
                }
            }
            handler.postDelayed(this, 1000)
        }
    }
    private val handler = Handler(Looper.getMainLooper())

    // F7-FR1: playback outlives re-renders; the card shows its state.
    private var playback: Playback? = null
    private var playlist: SegmentPlaylist? = null
    private var playPill: TextView? = null
    private var position: TextView? = null

    /** The position line, once a second while playing (NFR2). */
    private val tick = object : Runnable {
        override fun run() {
            showPosition()
            if (playback?.playing == true) handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        cards = MeetingCards(this, ui)
        id = intent.getStringExtra(EXTRA_ID) ?: run { finish(); return }
        cueAt = intent.getLongExtra(EXTRA_AT_MS, -1).takeIf { it >= 0 }
        focus = intent.getStringExtra(EXTRA_FOCUS)
        CoreJobs.init(filesDir.absolutePath)
        setContentView(ui.page(ui.column().apply { addView(ui.header("All meetings")) }))
    }

    override fun onResume() {
        super.onResume()
        // INV8: restored straight into this screen after the process died, a meeting may still
        // read transcribing with no job; recovery (cheap, queued first) moves it on.
        val alive = RecordingService.running
        CoreJobs.exec {
            deviceRecoverer().run(alive)?.let { StopJob.resume(it.resume) } // F15-AC8: catch up
        }
        // Settings and templates may have changed in their screens or on the desktop. CoreJobs
        // answers in order, so both are in hand when the meeting arrives: one redraw.
        scope.core({ NativeBridge.nativeGetSettings() }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { settings = it }
        }
        scope.core({ NativeBridge.nativeListTemplates() }) { json ->
            CoreJson.list(json, TemplateRow::from).getOrNull()?.let { templates = it }
        }
        load()
    }

    override fun onStart() {
        super.onStart()
        // A chunk's lines arrived (the lane's pass ended): show them now, not at the next poll.
        unLive = LiveJobs.observe { mid, _ -> if (mid == id) fetchLive() }
        // Stopped (here, on Home or in the notification): the meeting moves on; read it again.
        unHub = RecordingService.hub.observe { st ->
            if (view?.meeting?.status == Status.RECORDING && st is RecState.Idle) load()
        }
    }

    override fun onStop() {
        unLive?.invoke(); unLive = null
        unHub?.invoke(); unHub = null
        handler.removeCallbacks(livePoll)
        playback?.release() // leaving the screen stops the audio
        handler.removeCallbacks(tick)
        handler.removeCallbacks(poll) // the job carries on; onResume picks its progress up again
        showPlayer()
        super.onStop()
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** Re-reads the sidecar; the screen is rebuilt once per answer (one e-ink redraw). */
    private fun load() {
        scope.core({ NativeBridge.nativeGetMeeting(id) }) { json ->
            when (val r = CoreJson.obj(json, MeetingView::from)) {
                is CoreResult.Ok -> {
                    view = r.value; preparePlayback(r.value); cueOnce(r.value); render(); scrollToFocus()
                    handler.removeCallbacks(poll)
                    if (LongJobs.lane.isRunning(id)) handler.postDelayed(poll, 1000)
                    handler.removeCallbacks(livePoll)
                    if (recordingLive(r.value)) handler.post(livePoll)
                }
                is CoreResult.Err -> showGone(r.message)
            }
        }
    }

    /** F19-FR3: opened from a search hit at a moment: playback waits there, the paragraph spoken
     *  then is on the transcript page shown and outlined. Only on the first load. */
    private fun cueOnce(v: MeetingView) {
        val at = cueAt ?: return
        cueAt = null
        playback?.cue(at)
        TranscriptPages.locate(v.transcript, at)?.let { mark = it; transcriptPage = it.first }
    }

    /** F19-FR3: scrolled to the minutes, to-dos or transcript a hit came from, once. */
    private fun scrollToFocus() {
        val f = focus ?: return
        focus = null
        val target = anchors[f] ?: return
        page?.post { page?.scrollTo(0, target.top) }
    }

    /** Deleted on the desktop, or unreadable: say so rather than show stale details. */
    private fun showGone(message: String) {
        setContentView(ui.page(ui.column().apply {
            addView(ui.header("All meetings"))
            addView(ui.spacer(ui.dim(48)))
            addView(ui.note("This meeting can't be opened: $message", 16f).apply { gravity = Gravity.CENTER })
        }))
    }

    /** A new playlist only when the segments changed (a recording that grew, a file gone). */
    private fun preparePlayback(v: MeetingView) {
        val next = SegmentPlaylist.of(v)
        if (next.items == playlist?.items) return
        playback?.release()
        playlist = next
        playback = Playback(next, MediaPlayerPort()) { showPlayer() }
    }

    /** Rebuilds the page, keeping the scroll position (one redraw). */
    private fun show(column: LinearLayout) {
        val y = page?.scrollY ?: 0
        val p = ui.page(column)
        page = p
        setContentView(p)
        if (y > 0) p.post { p.scrollTo(0, y) }
    }

    /** F7-FR1: play/pause through every segment as one recording, with position and total. */
    private fun playerCard(): View = ui.card().apply {
        addView(ui.eyebrow("Recording"))
        val pill = ui.pill("▶  Play", size = 17f) { playback?.toggle() }
        val pos = ui.body("", 18f).apply { setPadding(ui.dim(20), 0, 0, 0) }
        playPill = pill; position = pos
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, ui.dim(12), 0, 0)
            addView(pill)
            addView(pos)
        })
        showPlayer()
    }

    private fun showPlayer() {
        val pb = playback ?: return
        val pill = playPill ?: return
        if (!pb.canPlay) {
            ui.stylePill(pill, "▶  Play", filled = false)
            pill.setTextColor(ui.textMuted); pill.isClickable = false
            position?.text = "No audio to play"
            return
        }
        ui.stylePill(pill, if (pb.playing) "❚❚  Pause" else "▶  Play", filled = pb.playing)
        showPosition()
        handler.removeCallbacks(tick)
        if (pb.playing) handler.postDelayed(tick, 1000)
    }

    private fun showPosition() {
        val pb = playback ?: return
        position?.text = "${Format.duration(pb.positionMs())}  /  ${Format.duration(pb.totalMs())}"
    }

    /** F11-FR6 / F7-FR2: the template the next Transcribe or Summarise again uses, with an inline picker. */
    private fun templateCard(v: MeetingView): View? {
        if (templates.isEmpty()) return null
        val choice = TemplateChoice.of(v.meeting.templateId, settings?.effectiveDefaultTemplate ?: TemplateChoice.GENERAL, templates)
        val current = choice.current ?: return null
        val canChange = v.meeting.status != Status.RECORDING && v.meeting.status != Status.TRANSCRIBING &&
            !LongJobs.lane.isRunning(id)
        return ui.card(padV = ui.dim(12)).apply {
            addView(LinearLayout(this@MeetingActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                addView(LinearLayout(this@MeetingActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(ui.eyebrow("Template"))
                    addView(ui.body(current.name, 18f, bold = true), ui.wrap().apply { topMargin = ui.dim(4) })
                    addView(ui.note(choice.note ?: current.description, 13f), ui.wrap().apply { topMargin = ui.dim(2) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                if (canChange) addView(ui.pill(if (pickingTemplate) "Done" else "Change", size = 15f) { pickingTemplate = !pickingTemplate; render() })
            })
            if (pickingTemplate && canChange) {
                addView(ui.spacer(ui.dim(8)))
                choice.options.forEach { t ->
                    addView(ui.hairline())
                    addView(ui.choiceRow(t.name, t.description, t.id == current.id) { setTemplate(t.id) })
                }
            }
        }
    }

    private fun setTemplate(templateId: String) {
        pickingTemplate = false
        scope.core({ NativeBridge.nativeSetMeetingTemplate(id, templateId) }) { json ->
            (CoreJson.ok(json) as? CoreResult.Err)?.let { InkDialog.error(this, "Couldn't change the template", it.message) }
            load()
        }
    }

    private fun actionsFor(m: Sidecar): Actions {
        val s = settings
        return Actions.of(m, LongJobs.lane.isRunning(id),
            s?.keys?.get(s.effectiveTranscribe.provider), s?.keys?.get(s.effectiveSummarise.provider))
    }

    /** F7-FR2: the one action the status allows, or progress with Cancel while a job runs. */
    private fun actionsRow(a: Actions): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        progress = null
        a.primary?.let { job ->
            val pill = ui.pill(job.label, filled = a.primaryEnabled) { if (a.primaryEnabled) confirmThenStart(job) }
            if (!a.primaryEnabled) { pill.setTextColor(ui.textMuted); pill.isClickable = false }
            val row = LinearLayout(this@MeetingActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                addView(pill, ui.wrap())
                // F5-FR13: a white pill beside the main action.
                a.secondary?.let { again ->
                    addView(ui.pill(again.label) { confirmAgain(again) }, ui.wrap().apply { marginStart = ui.dim(16) })
                }
            }
            addView(row, ui.wrap())
        }
        a.progress?.let { label ->
            val chip = ui.chip(label)
            progress = chip
            addView(chip, ui.wrap())
            if (a.canCancel) addView(ui.pill("Cancel", size = 15f) { cancel() }, ui.wrap().apply { topMargin = ui.dim(12) })
        }
        a.note?.let { addView(ui.note(it, 15f).apply { gravity = Gravity.CENTER }, ui.wrap().apply { topMargin = ui.dim(12) }) }
    }

    /** Transcribe again replaces the transcript, so it is asked first; to-dos and the title stay. */
    private fun confirmAgain(job: Actions.Job) {
        val what = if (view?.meeting?.nearLive == true) "Every part of the recording" else "Every segment"
        InkDialog.confirm(this, "Transcribe again?",
            "$what is sent again and the transcript replaced, then it is summarised again. To-dos, ticks and the title stay.",
            job.label) { confirmThenStart(job) }
    }

    /** Security: the first job to a provider names it once, and nothing is sent without OK. */
    private fun confirmThenStart(job: Actions.Job) {
        val notice = settings?.let { ProviderNotice.of(job, it) } ?: return start(job)
        InkDialog.notice(this, ProviderNotice.TITLE, notice.message, job.label) {
            scope.core({ NativeBridge.nativeSetSettings(SettingsPatch.noticeShown(notice.providers)) }) { json ->
                CoreJson.obj(json, SettingsView::from).getOrNull()?.let { settings = it }
                start(job)
            }
        }
    }

    /** Runs on the job lane; the result is already in the sidecar, so the screen just re-reads it. */
    private fun start(job: Actions.Job) {
        val chunked = view?.meeting?.nearLive == true
        val started = LongJobs.lane.run(id, {
            when (job) {
                // F15-FR9: a near-live meeting's gone chunk files are re-cut from its segments first.
                Actions.Job.TRANSCRIBE -> if (chunked) ChunkJob.device(again = false).run(id) else NativeBridge.nativeTranscribe(id)
                // F15-FR10: every chunk is re-cut from the segments, then sent again.
                Actions.Job.TRANSCRIBE_AGAIN -> if (chunked) ChunkJob.device(again = true).run(id) else NativeBridge.nativeTranscribeAgain(id)
                Actions.Job.SUMMARISE -> NativeBridge.nativeSummarise(id)
            }
        }) { json ->
            if (isDestroyed) return@run
            // Not started at all (busy, no key, not offered now): say why. A job that ran and
            // failed shows its error under the action instead.
            val r = CoreJson.obj(json, Sidecar::ofJob)
            if (r is CoreResult.Err) InkDialog.error(this, "${job.label} didn't start", r.message)
            load()
        }
        if (started) load()
    }

    /** A job still waiting behind another is just taken off the lane (N9); a running one is the core's. */
    private fun cancel() {
        if (LongJobs.lane.cancelQueued(id)) return load()
        scope.core({ NativeBridge.nativeCancel(id) }) { load() }
    }

    private fun render() {
        val v = view ?: return
        actions = actionsFor(v.meeting)
        val h = MeetingHeader.of(v.meeting, v.cost)
        val column = ui.column().apply {
            addView(ui.header("All meetings"))
            addView(ui.spacer(ui.dim(36)))
            // F7-FR5: the title is the heading; tapping it renames the meeting.
            addView(ui.body(h.title, 30f, bold = !h.untitled).apply {
                gravity = Gravity.CENTER
                if (h.untitled) { setTextColor(ui.textMuted); typeface = ui.serifItalic }
                isClickable = true
                setOnClickListener { rename(if (h.untitled) "" else h.title) }
            }, ui.fullWidth())
            if (h.stamp.isNotEmpty()) addView(ui.eyebrow(h.stamp).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(10), 0, 0) }, ui.fullWidth())
            // F16-FR5: what the meeting has cost so far, beside its length.
            addView(ui.eyebrow("${h.length}  ·  ${h.cost}").apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(4), 0, 0) }, ui.fullWidth())
            h.mic?.let { addView(ui.note(it).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(6), 0, 0) }, ui.fullWidth()) }
            // F18-FR10: "Linked to … · change" or "Link to event…". M12 hook: in layout B this
            // moves to the Minutes tab's header.
            addView(linkLine(v.meeting), ui.fullWidth())
            h.note?.let { addView(ui.note(it, 13f, ui.textMuted).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(2), 0, 0) }, ui.fullWidth()) }

            addView(ui.spacer(ui.dim(32)))
            addView(playerCard())
            templateCard(v)?.let { addView(ui.spacer(ui.dim(20))); addView(it) }
            addView(ui.spacer(ui.dim(24)))
            addView(actionsRow(actions!!))

            val m = v.meeting
            anchors.clear()
            SummaryCard.of(m)?.let { c ->
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("Summary").also { anchors["summary"] = it })
                addView(ui.spacer(ui.dim(14)))
                addView(cards.summary(c))
            }
            if (m.todos.isNotEmpty() || m.status == Status.DONE) {
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("To-dos from this meeting").also { anchors["todos"] = it })
                addView(ui.spacer(ui.dim(14)))
                addView(cards.todos(m.todos, ::setDone))
            }
            if (recordingLive(v)) {
                addView(ui.spacer(ui.dim(28)))
                addView(liveCard())
            }
            val pages = if (recordingLive(v)) emptyList() else TranscriptPages.pages(TranscriptPages.paragraphs(v.transcript))
            if (pages.isNotEmpty()) {
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("Transcript").also { anchors["transcript"] = it })
                addView(ui.spacer(ui.dim(14)))
                addView(cards.transcript(pages, transcriptPage, mark) { transcriptPage = it })
            }

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
            if (actions?.canDelete == true) {
                addView(ui.eyebrow("Delete meeting", ui.textMuted).apply {
                    gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, ui.dim(20))
                    isClickable = true
                    setOnClickListener { delete(v) }
                }, ui.fullWidth())
            }
            addView(ui.closingMark())
        }
        show(column)
    }

    /** F7-FR3: tick or untick; a ticked to-do stays ticked through later summaries (F5-FR5). */
    private fun setDone(t: Todo) {
        scope.core({ NativeBridge.nativeSetTodoDone(id, t.id, !t.done) }) { json ->
            (CoreJson.ok(json) as? CoreResult.Err)?.let { InkDialog.error(this, "Couldn't change the to-do", it.message) }
            load()
        }
    }

    /** F7-FR4: after confirmation, the core removes the audio, sidecar and transcript. */
    private fun delete(v: MeetingView) {
        val what = Format.plural(v.meeting.segments.size, "audio file")
        InkDialog.confirm(this, "Delete this meeting?",
            "Its $what, transcript and to-dos are removed from ${v.dir.substringAfter("/storage/emulated/0/")}. This can't be undone.",
            "Delete") {
            playback?.release()
            scope.core({ NativeBridge.nativeDeleteMeeting(id) }) { json ->
                when (val r = CoreJson.ok(json)) {
                    is CoreResult.Ok -> finish()
                    is CoreResult.Err -> InkDialog.error(this, "Couldn't delete it", r.message)
                }
            }
        }
    }

    private fun linkLine(m: Sidecar): View = ui.eyebrow(
        if (m.eventKey != null) "Linked to ${m.eventTitle?.ifBlank { null } ?: "an event"}  ·  change" else "Link to event…",
        ui.inkSoft,
    ).apply {
        gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, ui.dim(4))
        isClickable = true
        setOnClickListener {
            val start = try { java.time.OffsetDateTime.parse(m.startedAt).toInstant().toEpochMilli() } catch (e: Exception) { return@setOnClickListener }
            EventLinkPicker.show(this@MeetingActivity, this@MeetingActivity.id, start, start + m.durationMs, m.eventKey) { load() }
        }
    }

    private fun recordingLive(v: MeetingView) = v.meeting.status == Status.RECORDING && v.meeting.nearLive

    /**
     * F15-FR6 / F17-FR6 on today's meeting screen (layout B's strip is M12): the newest three
     * lines, whole lines only, with "Offline · 4 waiting" or "Transcribing…" at the right.
     * Tapping it shows every line so far; tapping again, the newest three.
     */
    private fun liveCard(): View = ui.card().apply {
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.eyebrow("Live transcript"), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.eyebrow("", ui.inkSoft).also { liveStatus = it })
        })
        addView(LinearLayout(this@MeetingActivity).apply { orientation = LinearLayout.VERTICAL }.also { liveLines = it },
            ui.fullWidth().apply { topMargin = ui.dim(10) })
        isClickable = true
        setOnClickListener { liveAll = !liveAll; showLive() }
        showLive()
    }

    /** One read of the live transcript; the strip redraws only if it changed. */
    private fun fetchLive() {
        scope.core({ NativeBridge.nativeLiveTranscript(id, 0) }) { json ->
            if (json == liveJson) return@core
            liveJson = json
            live = CoreJson.obj(json, LiveTranscript::from).getOrNull() ?: return@core
            showLive()
        }
    }

    private fun showLive() {
        val box = liveLines ?: return
        val t = live
        liveStatus?.text = t?.status.orEmpty().uppercase()
        box.removeAllViews()
        val lines = t?.shown(liveAll).orEmpty()
        if (lines.isEmpty()) {
            box.addView(ui.note(LiveTranscript.EMPTY, 15f, ui.textMuted))
            return
        }
        lines.forEach { l ->
            box.addView(ui.body(l.text, 17f).apply { setLineSpacing(0f, 1.3f) }, ui.fullWidth().apply { topMargin = ui.dim(6) })
        }
    }

    /** F7-FR5: an empty title returns the meeting to untitled; the next summary may name it. */
    private fun rename(current: String) {
        InkDialog.text(this, ui, "Rename meeting", current) { title ->
            scope.core({ NativeBridge.nativeRenameMeeting(id, title) }) { json ->
                (CoreJson.ok(json) as? CoreResult.Err)?.let { InkDialog.error(this, "Couldn't rename it", it.message) }
                load()
            }
        }
    }

    companion object {
        const val EXTRA_ID = "meeting_id"
        /** F19-FR3: recording-clock ms to cue playback to (a search hit's time). */
        const val EXTRA_AT_MS = "at_ms"
        /** F19-FR3: summary, todos or transcript: the part to scroll to. */
        const val EXTRA_FOCUS = "focus"
        /** How often the live transcript is read while recording, besides the lane's own news. */
        const val LIVE_POLL_MS = 5_000L
    }
}
