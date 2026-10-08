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
import dev.jraghavan.snmeeting.model.MeetingHeader
import dev.jraghavan.snmeeting.model.MeetingView
import dev.jraghavan.snmeeting.model.ProviderNotice
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.Sidecar
import dev.jraghavan.snmeeting.model.Status
import dev.jraghavan.snmeeting.model.TemplateChoice
import dev.jraghavan.snmeeting.model.TemplateRow
import dev.jraghavan.snmeeting.model.Todo
import dev.jraghavan.snmeeting.model.TranscriptPages
import dev.jraghavan.snmeeting.play.MediaPlayerPort
import dev.jraghavan.snmeeting.play.Playback
import dev.jraghavan.snmeeting.play.SegmentPlaylist

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
        CoreJobs.init(filesDir.absolutePath)
        setContentView(ui.page(ui.column().apply { addView(ui.header("All meetings")) }))
    }

    override fun onResume() {
        super.onResume()
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

    override fun onStop() {
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
                    view = r.value; preparePlayback(r.value); render()
                    handler.removeCallbacks(poll)
                    if (LongJobs.lane.isRunning(id)) handler.postDelayed(poll, 1000)
                }
                is CoreResult.Err -> showGone(r.message)
            }
        }
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
        position?.text = "${Format.duration(pb.positionMs())}  /  ${Format.duration(playlist?.totalMs ?: 0)}"
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
            addView(pill, ui.wrap())
        }
        a.progress?.let { label ->
            val chip = ui.chip(label)
            progress = chip
            addView(chip, ui.wrap())
            if (a.canCancel) addView(ui.pill("Cancel", size = 15f) { cancel() }, ui.wrap().apply { topMargin = ui.dim(12) })
        }
        a.note?.let { addView(ui.note(it, 15f).apply { gravity = Gravity.CENTER }, ui.wrap().apply { topMargin = ui.dim(12) }) }
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
        val started = LongJobs.lane.run(id, {
            if (job == Actions.Job.TRANSCRIBE) NativeBridge.nativeTranscribe(id) else NativeBridge.nativeSummarise(id)
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

    private fun cancel() {
        scope.core({ NativeBridge.nativeCancel(id) }) { load() }
    }

    private fun render() {
        val v = view ?: return
        actions = actionsFor(v.meeting)
        val h = MeetingHeader.of(v.meeting)
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
            addView(ui.eyebrow(h.length).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(4), 0, 0) }, ui.fullWidth())
            h.mic?.let { addView(ui.note(it).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(6), 0, 0) }, ui.fullWidth()) }
            h.note?.let { addView(ui.note(it, 13f, ui.textMuted).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(2), 0, 0) }, ui.fullWidth()) }

            addView(ui.spacer(ui.dim(32)))
            addView(playerCard())
            templateCard(v)?.let { addView(ui.spacer(ui.dim(20))); addView(it) }
            addView(ui.spacer(ui.dim(24)))
            addView(actionsRow(actions!!))

            val m = v.meeting
            if (m.todos.isNotEmpty() || m.status == Status.DONE) {
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("To-dos from this meeting"))
                addView(ui.spacer(ui.dim(14)))
                addView(cards.todos(m.todos, ::setDone))
            }
            val pages = TranscriptPages.pages(TranscriptPages.paragraphs(m.segments))
            if (pages.isNotEmpty()) {
                addView(ui.spacer(ui.dim(36)))
                addView(ui.eyebrowItalic("Transcript"))
                addView(ui.spacer(ui.dim(14)))
                addView(cards.transcript(pages, transcriptPage) { transcriptPage = it })
            }

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
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
    }
}
