package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.InkDialog
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.ScreenScope
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.record.LiveClock
import org.json.JSONObject

/**
 * DEVICE-UNVERIFIED (wiring of views; the rules are in the pure classes it uses). Layout B's
 * Notes tab (F17-FR1): the infinite canvas with its zoom pill and minimap. Strokes go to the
 * core as they end; the core keeps them in memory and this page flushes them on a short
 * deferred timer ([SAVE_MS], inkread's 1 500 ms) and when it closes, with the viewport.
 */
class NotesPage(
    private val a: Activity,
    private val ui: Inkwell,
    private val scope: ScreenScope,
    private val id: String,
    private val host: Host,
) {
    interface Host {
        /** The recording under way is this meeting's: its start (elapsed ms), else null. */
        fun recordingSince(): Long?
    }

    private val handler = Handler(Looper.getMainLooper())
    private val save = Runnable { scope.core({ NativeBridge.nativeInkSave(id) }) { json -> reportError(json, "Couldn't save the notes") } }
    private var opened = false
    /** The Notes tab is in front: only then is the pen claimed for firmware ink. */
    private var front = true

    var tool: NoteTool = NoteTool.PEN

    val canvas = NotesCanvasView(a, object : NotesCanvasView.Host {
        override val tool get() = this@NotesPage.tool
        override val recording get() = host.recordingSince() != null
        override val penWidthPx get() = PEN_WIDTH_PX

        override fun strokeTimes(): StrokeTimes {
            val since = host.recordingSince() ?: return StrokeTimes.UNTIMED
            return StrokeTimes.recording(SystemClock.uptimeMillis(), SystemClock.elapsedRealtime(), since) { LiveClock.current }
        }

        override fun commitStroke(tool: NoteTool, widthWorld: Float, xypt: FloatArray) = changed {
            NativeBridge.nativeInkStroke(id, LAYER, tool.core, widthWorld, xypt)
        }

        override fun erase(xy: FloatArray, radiusWorld: Float) = changed { NativeBridge.nativeInkErase(id, LAYER, xy, radiusWorld) }
        override fun lasso(xy: FloatArray) = Unit
        override fun moveSelection(dx: Float, dy: Float) = Unit
        override fun listen(x: Float, y: Float, radiusWorld: Float) = Unit
        override fun viewportChanged(v: Viewport) = fetch(redraw = true)
    })

    private val zoomPill = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        background = ui.outline(ui.dim(30))
        setPadding(ui.dim(8), ui.dim(4), ui.dim(8), ui.dim(4))
        val item = { label: String, act: () -> Unit ->
            addView(ui.eyebrow(label).apply {
                setPadding(ui.dim(12), ui.dim(10), ui.dim(12), ui.dim(10))
                isClickable = true; setOnClickListener { act() }
            })
        }
        item("FIT") { canvas.fit() }
        Viewport.PRESETS.forEach { p -> item("$p%") { canvas.zoomPercent(p) } }
    }

    /** The canvas with the zoom pill over its top corner. */
    val canvasFrame = FrameLayout(a).apply {
        addView(canvas, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(zoomPill, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.TOP or Gravity.START).apply { topMargin = ui.dim(12); marginStart = ui.dim(12) })
    }

    /** The tab's content: the canvas column (the bar and strip join it in later steps). */
    val column = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        addView(canvasFrame, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    val view: View get() = column

    /** Opens the canvas at its last viewport, or fitted to its ink. */
    fun open() {
        scope.core({ NativeBridge.nativeNotesOpen(id) }) { json ->
            when (val r = CoreJson.obj(json) { it }) {
                is CoreResult.Ok -> {
                    opened = true
                    val o = r.value
                    canvas.post {
                        val w = canvas.width.toDouble(); val h = canvas.height.toDouble()
                        val v = o.optJSONObject("view")
                        val next = if (v != null) {
                            Viewport.centred(v.optDouble("cx"), v.optDouble("cy"), v.optDouble("zoom", 1.0), w, h)
                        } else {
                            Viewport.fitInk(InkView.rect(o.optJSONArray("bounds")), w, h, NotesCanvasView.FIT_PAD)
                        }
                        canvas.setViewport(next)
                        if (front) canvas.claimPen()
                    }
                }
                is CoreResult.Err -> InkDialog.error(a, "The notes can't be opened", r.message)
            }
        }
    }

    /** The tab came to the front or went behind the Minutes tab. */
    fun shown(visible: Boolean) {
        if (visible == front) return
        front = visible
        if (!opened) return
        if (visible) canvas.claimPen() else canvas.releasePen()
    }

    /** The page goes: the pen is let go, the ink saved and the viewport kept. */
    fun close() {
        handler.removeCallbacks(save)
        canvas.releasePen()
        if (!opened) return
        opened = false
        val (cx, cy) = canvas.viewport.centre(canvas.width.toDouble(), canvas.height.toDouble())
        val z = canvas.viewport.zoom
        scope.core({ NativeBridge.nativeNotesClose(id, cx.toFloat(), cy.toFloat(), z.toFloat()) }) { json ->
            reportError(json, "Couldn't save the notes")
        }
    }

    /** A change to the ink: run [call], fetch what is in view (drawn once the pen rests) and
     *  arm the deferred save. */
    private fun changed(call: () -> String) {
        scope.core(call) { json ->
            if (!reportError(json, "Couldn't keep that")) {
                fetch(redraw = false)
                handler.removeCallbacks(save)
                handler.postDelayed(save, SAVE_MS)
            }
        }
    }

    /** The strokes in view (plus a margin, so a small pan needs no fetch). */
    fun fetch(redraw: Boolean) {
        val v = canvas.viewport
        val r = v.visible(canvas.width.toDouble(), canvas.height.toDouble())
        val m = maxOf(r.width, r.height) / 2
        scope.core({
            NativeBridge.nativeInkStrokesForDraw(id, LAYER, (r.left - m).toFloat(), (r.top - m).toFloat(),
                (r.right + m).toFloat(), (r.bottom + m).toFloat())
        }) { json ->
            CoreJson.obj(json, InkView::from).getOrNull()?.let { canvas.show(it, redraw) }
        }
    }

    /** True when [json] is an error, which is then shown. */
    private fun reportError(json: String, title: String): Boolean {
        val e = CoreJson.obj(json) { o: JSONObject -> o } as? CoreResult.Err ?: return false
        InkDialog.error(a, title, e.message)
        return true
    }

    companion object {
        const val LAYER = "canvas"
        const val SAVE_MS = 1_500L
        /** inkread's default nib (`StylusInkController.PEN_WIDTHS[2]`): matches the firmware's. */
        const val PEN_WIDTH_PX = 9f
    }
}
