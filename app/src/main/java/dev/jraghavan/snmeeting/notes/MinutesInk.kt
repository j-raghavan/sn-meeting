package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.InkDialog
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.ScreenScope
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import org.json.JSONObject

/**
 * DEVICE-UNVERIFIED (pen in a scrolling page on the Manta). F17-FR10: "Your notes on the
 * minutes", an ink area at the end of the Minutes tab, stored as its own layer
 * (`minutes.inkbin`). The pen writes (an inverted pen erases); a finger scrolls the page. No
 * provider result ever writes it (INV12). Its strokes are untimed: the minutes come after.
 */
class MinutesInk(private val a: Activity, private val ui: Inkwell, private val scope: ScreenScope, private val id: String) {
    private val handler = Handler(Looper.getMainLooper())
    private val save = Runnable { scope.core({ NativeBridge.nativeInkSave(id) }) { } }

    val canvas: NotesCanvasView = NotesCanvasView(a, object : NotesCanvasView.Host {
        override val tool = NoteTool.PEN
        override val recording = false
        override val penWidthPx = NotesPage.PEN_WIDTH_PX
        override fun strokeTimes() = StrokeTimes.UNTIMED
        override fun commitStroke(tool: NoteTool, widthWorld: Float, xypt: FloatArray) =
            changed { NativeBridge.nativeInkStroke(id, LAYER, tool.core, widthWorld, xypt) }
        override fun erase(xy: FloatArray, radiusWorld: Float) = changed { NativeBridge.nativeInkErase(id, LAYER, xy, radiusWorld) }
        override fun lasso(xy: FloatArray) = Unit
        override fun moveSelection(dx: Float, dy: Float) = Unit
        override fun listen(x: Float, y: Float, radiusWorld: Float) = Unit
        override fun viewportChanged(v: Viewport) = Unit
    }, fixed = true)

    /** The sheet, [HEIGHT] design units tall, framed. */
    val view = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        background = ui.outline(ui.dim(6))
        addView(canvas, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, ui.dim(HEIGHT)))
    }

    fun load() = fetch()

    /** The page goes: anything waiting is saved. */
    fun close() {
        handler.removeCallbacks(save)
        scope.core({ NativeBridge.nativeInkSave(id) }) { }
    }

    private fun changed(call: () -> String) = scope.core(call) { json ->
        val err = CoreJson.obj(json) { o: JSONObject -> o } as? CoreResult.Err
        if (err != null) return@core InkDialog.error(a, "Couldn't keep that", err.message)
        fetch()
        handler.removeCallbacks(save)
        handler.postDelayed(save, NotesPage.SAVE_MS)
    }

    private fun fetch() = scope.core({ NativeBridge.nativeInkStrokesForDraw(id, LAYER, -1e6f, -1e6f, 1e6f, 1e6f) }) { json ->
        CoreJson.obj(json, InkView::from).getOrNull()?.let { canvas.show(it) }
    }

    companion object {
        const val LAYER = "minutes"
        const val HEIGHT = 420
    }
}
