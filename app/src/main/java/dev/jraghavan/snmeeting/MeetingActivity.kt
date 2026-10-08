package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.MeetingHeader
import dev.jraghavan.snmeeting.model.MeetingView

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

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** Re-reads the sidecar; the screen is rebuilt once per answer (one e-ink redraw). */
    private fun load() {
        scope.core({ NativeBridge.nativeGetMeeting(id) }) { json ->
            when (val r = CoreJson.obj(json, MeetingView::from)) {
                is CoreResult.Ok -> { view = r.value; render() }
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

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
            addView(ui.closingMark())
        }
        setContentView(ui.page(column))
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
