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
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.Format
import dev.jraghavan.snmeeting.model.MeetingHeader
import dev.jraghavan.snmeeting.model.MeetingView
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
        id = intent.getStringExtra(EXTRA_ID) ?: run { finish(); return }
        CoreJobs.init(filesDir.absolutePath)
        setContentView(ui.page(ui.column().apply { addView(ui.header("All meetings")) }))
    }

    override fun onResume() {
        super.onResume()
        load()
    }

    override fun onStop() {
        playback?.release() // leaving the screen stops the audio
        handler.removeCallbacks(tick)
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
                is CoreResult.Ok -> { view = r.value; preparePlayback(r.value); render() }
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

    private fun render() {
        val v = view ?: return
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

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
            addView(ui.closingMark())
        }
        show(column)
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
