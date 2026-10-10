package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.Actions
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.Format
import dev.jraghavan.snmeeting.model.LiveTranscript
import dev.jraghavan.snmeeting.model.Mark
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
import dev.jraghavan.snmeeting.notes.NotesPage
import dev.jraghavan.snmeeting.play.MediaPlayerPort
import dev.jraghavan.snmeeting.play.Playback
import dev.jraghavan.snmeeting.play.SegmentPlaylist
import dev.jraghavan.snmeeting.record.deviceRecoverer
import dev.jraghavan.snmeeting.record.LiveClock
import dev.jraghavan.snmeeting.record.RecState
import dev.jraghavan.snmeeting.record.RecordingService

/**
 * One meeting in layout B, "Notebook with Strip" (F17-FR1), Inkwell style (F9). The top bar
 * holds the Notes | Minutes tabs and, at the right, the recording timer (or the date and
 * length). Notes is the infinite canvas ([NotesPage]); Minutes is F7's page, read from the
 * core: the title (tap to rename), playback, the template, the action its status allows, the
 * summary, to-dos and the transcript paged for e-ink. A recording opens on Notes, a past
 * meeting on Minutes.
 */
class MeetingActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    private lateinit var id: String
    private var view: MeetingView? = null
    /** F18-FR10: the meeting's day in the calendar, for whether "Link to event…" shows. */
    private var linkDay: dev.jraghavan.snmeeting.model.Agenda? = null
    private lateinit var page: ScrollView
    private lateinit var notesHost: FrameLayout
    private lateinit var tabNotes: TextView
    private lateinit var tabMinutes: TextView
    private lateinit var topTimer: TextView
    /** F17-FR1: the tab shown; chosen once, from the meeting's status, on the first load. */
    private var tab: String? = null
    private var notes: NotesPage? = null
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

    /** F17-FR1 / FR11: the top bar's timer, once a second while recording, its own view only. */
    private val clock = object : Runnable {
        override fun run() {
            showClock()
            if (recordingHere() != null) handler.postDelayed(this, 1000)
        }
    }

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
        tab = intent.getStringExtra(EXTRA_TAB)
        CoreJobs.init(filesDir.absolutePath)
        setContentView(skeleton())
    }

    /** F17-FR1: the top bar (tabs at the left, timer at the right) over the two tabs' area. */
    private fun skeleton(): View {
        tabNotes = tabLabel("Notes", NOTES)
        tabMinutes = tabLabel("Minutes", MINUTES)
        topTimer = ui.eyebrow("").apply { gravity = Gravity.END }
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(ui.side, ui.dim(18), ui.side, ui.dim(14))
            addView(LinearLayout(this@MeetingActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                addView(ui.eyebrow("←").apply {
                    isClickable = true; setOnClickListener { finish() }
                    setPadding(0, ui.dim(8), ui.dim(18), ui.dim(8))
                })
                addView(tabNotes)
                addView(ui.eyebrow("|", ui.textMuted).apply { setPadding(ui.dim(12), 0, ui.dim(12), 0) })
                addView(tabMinutes)
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.wordmark("Meetings", 16f))
            addView(topTimer, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
        page = ui.page(ui.column())
        notesHost = FrameLayout(this).apply { visibility = View.GONE }
        val body = FrameLayout(this).apply {
            addView(page, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            addView(notesHost, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(android.graphics.Color.WHITE)
            addView(bar, ui.fullWidth())
            addView(ui.hairline())
            addView(body, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        }
    }

    private fun tabLabel(text: String, which: String): TextView = ui.body(text, 19f).apply {
        isClickable = true
        setOnClickListener { if (tab != which) { tab = which; showTab() } }
        setPadding(0, ui.dim(8), 0, ui.dim(8))
    }

    /** F17-FR1: one tab or the other, as one redraw. The canvas is built the first time. */
    private fun showTab() {
        val onNotes = tab == NOTES
        for ((label, mine) in listOf(tabNotes to NOTES, tabMinutes to MINUTES)) {
            val on = tab == mine
            label.typeface = if (on) ui.serifBold else ui.serif
            label.setTextColor(if (on) ui.ink else ui.textMuted)
            label.paintFlags = if (on) label.paintFlags or android.graphics.Paint.UNDERLINE_TEXT_FLAG
            else label.paintFlags and android.graphics.Paint.UNDERLINE_TEXT_FLAG.inv()
        }
        page.visibility = if (onNotes) View.GONE else View.VISIBLE
        notesHost.visibility = if (onNotes) View.VISIBLE else View.GONE
        notes?.shown(onNotes)
        if (onNotes && notes == null) {
            notes = NotesPage(this, ui, scope, id, object : NotesPage.Host {
                override fun recordingSince(): Long? = recordingHere()
                override fun prefs() = settings?.notes ?: dev.jraghavan.snmeeting.notes.NotesPrefs()
                override fun savePrefs(patch: String) = scope.core({ NativeBridge.nativeSetSettings(patch) }) { json ->
                    CoreJson.obj(json, SettingsView::from).getOrNull()?.let { settings = it }
                }
                override fun timer(): String = clockText()
                override fun cost(): String? = live?.soFar?.takeIf { view?.meeting?.nearLive == true }
                override fun playing() = playback?.playing == true
                override fun canPlay() = playback?.canPlay == true && recordingHere() == null
                override fun togglePlay() { playback?.toggle() }
                override fun mark() { this@MeetingActivity.mark() }
                override fun stop() { if (RecordingService.hub.state is RecState.Recording) RecordingService.stop(this@MeetingActivity) }
            }).also {
                notesHost.addView(it.view, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
                it.liveAvailable = view?.meeting?.nearLive == true
                live?.let(it::showLive)
                it.setMarks(view?.meeting?.marks.orEmpty().map { m -> m.atMs })
                it.open()
            }
        }
    }

    /** When this meeting's recording began (elapsed ms), while it is the one recording. */
    private fun recordingHere(): Long? =
        (RecordingService.hub.state as? RecState.Recording)?.takeIf { it.id == id }?.startElapsedMs

    /** "● 0:12:41" while recording; else the meeting's date and length (F17-FR1). */
    private fun showClock() {
        val since = recordingHere()
        val text = if (since != null) {
            "●  " + clockText()
        } else {
            view?.let { v -> MeetingHeader.of(v.meeting, v.cost).let { h -> listOf(h.stamp, h.length).filter { it.isNotEmpty() }.joinToString("  ·  ") } }.orEmpty()
        }
        if (topTimer.text.toString() != text) topTimer.text = text
        notes?.bar?.showTimer(clockText()) // the bar's timer: its own view only (F17-FR11)
    }

    /** The recording's time, or the playback position when not recording. */
    private fun clockText(): String {
        val since = recordingHere()
        if (since != null) return Format.duration(LiveClock.nowMs(android.os.SystemClock.elapsedRealtime(), since))
        return playback?.let { Format.duration(it.positionMs()) } ?: ""
    }

    override fun onResume() {
        super.onResume()
        // INV8: restored straight into this screen after the process died, a meeting may still
        // read transcribing with no job; recovery (cheap, queued first) moves it on.
        val alive = RecordingService.running
        CoreJobs.exec {
            deviceRecoverer(filesDir).run(alive)?.let { StopJob.resume(it.resume) } // F15-AC8: catch up
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
        notes?.open() // the canvas was let go in onStop
        // A chunk's lines arrived (the lane's pass ended): show them now, not at the next poll.
        unLive = LiveJobs.observe { mid, _ -> if (mid == id) fetchLive() }
        // Stopped (here, on Home or in the notification): the meeting moves on; read it again.
        unHub = RecordingService.hub.observe { st ->
            if (view?.meeting?.status == Status.RECORDING && st is RecState.Idle) load()
        }
    }

    override fun onStop() {
        notes?.close() // F17-FR5: the ink saved and the viewport kept; the pen given back
        handler.removeCallbacks(clock)
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
                    loadLinkDay(r.value.meeting)
                    if (tab == null) tab = if (r.value.meeting.status == Status.RECORDING) NOTES else MINUTES
                    showTab()
                    handler.removeCallbacks(clock)
                    handler.post(clock)
                    handler.removeCallbacks(poll)
                    if (LongJobs.lane.isRunning(id)) handler.postDelayed(poll, 1000)
                    handler.removeCallbacks(livePoll)
                    if (recordingLive(r.value)) handler.post(livePoll)
                    else if (r.value.meeting.nearLive) fetchLive() // a past meeting's lines, for the strip
                    notes?.liveAvailable = r.value.meeting.nearLive
                    notes?.setMarks(r.value.meeting.marks.map { it.atMs })
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
        TranscriptPages.locate(v.transcript, at, marks = v.meeting.marks)?.let { mark = it; transcriptPage = it.first }
    }

    /** F19-FR3: scrolled to the minutes, to-dos or transcript a hit came from, once. */
    private fun scrollToFocus() {
        val f = focus ?: return
        focus = null
        val target = anchors[f] ?: return
        page.post { page.scrollTo(0, target.top) }
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

    /** Rebuilds the Minutes tab, keeping its scroll position (one redraw). */
    private fun show(column: LinearLayout) {
        val y = page.scrollY
        page.removeAllViews()
        page.addView(column)
        if (y > 0) page.post { page.scrollTo(0, y) }
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
            s?.keys?.get(s.effectiveTranscribe.provider), s?.keys?.get(s.effectiveSummarise.provider),
            LongJobs.lane.elapsedMs(id))
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
            linkLine(v.meeting)?.let { addView(it, ui.fullWidth()) }
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
            if (m.marks.isNotEmpty() && m.status != Status.RECORDING) {
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("Marks"))
                addView(ui.spacer(ui.dim(14)))
                addView(marksCard(m.marks))
            }
            val pages = if (recordingLive(v)) emptyList() else TranscriptPages.pages(TranscriptPages.paragraphs(v.transcript, m.marks))
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

    /** Reads the meeting's day once (no network: the calendar cache); the line redraws if it changes. */
    private fun loadLinkDay(m: Sidecar) {
        val day = try { java.time.OffsetDateTime.parse(m.startedAt).toLocalDate().toString() } catch (e: Exception) { return }
        scope.core({ NativeBridge.nativeAgenda(day, day) }) { json ->
            val a = CoreJson.obj(json, dev.jraghavan.snmeeting.model.Agenda::from).getOrNull() ?: return@core
            val before = view?.meeting?.let { linkLabel(it) }
            linkDay = a
            if (view?.meeting?.let { linkLabel(it) } != before) render()
        }
    }

    private fun linkLabel(m: Sidecar): String? = dev.jraghavan.snmeeting.model.CalendarFormat.meetingLink(
        m.eventKey, m.eventTitle, linkDay?.hasLinks ?: false, linkDay?.days?.firstOrNull()?.rows.orEmpty())

    private fun linkLine(m: Sidecar): View? {
        val label = linkLabel(m) ?: return null
        return ui.eyebrow(label, ui.inkSoft).apply {
            gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, ui.dim(4))
            isClickable = true
            setOnClickListener {
                val start = try { java.time.OffsetDateTime.parse(m.startedAt).toInstant().toEpochMilli() } catch (e: Exception) { return@setOnClickListener }
                EventLinkPicker.show(this@MeetingActivity, this@MeetingActivity.id, start, start + m.durationMs, m.eventKey) { load() }
            }
        }
    }

    /** F17-FR3: Mark in the notes bar stamps the recording clock and reads "Marked 0:12:41"
     *  for 2 s, redrawing only the button; the ◆ goes beside the latest stroke. */
    private fun mark() {
        val rec = RecordingService.hub.state as? RecState.Recording ?: return
        if (rec.id != id) return
        val at = LiveClock.nowMs(android.os.SystemClock.elapsedRealtime(), rec.startElapsedMs)
        notes?.bar?.showMarked("Marked\n${Format.duration(at)}")
        handler.postDelayed({
            notes?.bar?.showMarked(dev.jraghavan.snmeeting.notes.ToolBarModel.MARK)
        }, MARKED_MS)
        scope.core({ NativeBridge.nativeAddMark(id, at) }) { json ->
            (CoreJson.ok(json) as? CoreResult.Err)?.let { InkDialog.error(this, "Couldn't add the mark", it.message) }
            fetchLive()
            notes?.setMarks(view?.meeting?.marks.orEmpty().map { it.atMs } + at) // ◆ beside the latest stroke
        }
    }

    /** F15-FR11: each mark; tap to give it a label (empty clears it), long-press to delete it. */
    private fun marksCard(marks: List<Mark>): View = ui.card(padV = ui.dim(10)).apply {
        val meetingId = this@MeetingActivity.id // `id` inside a view's apply is the view's own
        marks.forEachIndexed { i, mk ->
            if (i > 0) addView(ui.hairline())
            addView(ui.body(mk.line, 17f).apply {
                setPadding(0, ui.dim(12), 0, ui.dim(12))
                isClickable = true
                setOnClickListener {
                    InkDialog.text(this@MeetingActivity, ui, "Label this mark", mk.label.orEmpty()) { label ->
                        scope.core({ NativeBridge.nativeSetMarkLabel(meetingId, mk.id, label) }) { load() }
                    }
                }
                setOnLongClickListener {
                    InkDialog.confirm(this@MeetingActivity, "Delete this mark?", mk.line, "Delete") {
                        scope.core({ NativeBridge.nativeDeleteMark(meetingId, mk.id) }) { load() }
                    }
                    true
                }
            }, ui.fullWidth())
        }
    }

    private fun recordingLive(v: MeetingView) = v.meeting.status == Status.RECORDING && v.meeting.nearLive

    /** One read of the live transcript; the strip redraws only if it changed. */
    private fun fetchLive() {
        scope.core({ NativeBridge.nativeLiveTranscript(id, 0) }) { json ->
            if (json == liveJson) return@core
            liveJson = json
            live = CoreJson.obj(json, LiveTranscript::from).getOrNull() ?: return@core
            showLive()
            live?.let { notes?.showLive(it) } // F17-FR6: the Notes tab's strip
        }
    }

    /** F16-FR5: the cost line, redrawn only when its text changes (the strip is the Notes tab's). */
    private fun showLive() {
        if (view?.meeting?.nearLive == true) live?.soFar?.let { notes?.bar?.showCost(it) }
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
        /** F17-FR1 / F19-FR3: the tab to open on, [NOTES] or [MINUTES]; by status when absent. */
        const val EXTRA_TAB = "tab"
        const val NOTES = "notes"
        const val MINUTES = "minutes"
        /** How often the live transcript is read while recording, besides the lane's own news. */
        const val LIVE_POLL_MS = 5_000L
        /** F17-FR3: how long Mark reads "Marked 0:12:41". */
        const val MARKED_MS = 2_000L
    }
}
