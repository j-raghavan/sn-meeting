package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.model.arr
import dev.jraghavan.snmeeting.model.bool
import dev.jraghavan.snmeeting.model.int
import dev.jraghavan.snmeeting.model.long
import dev.jraghavan.snmeeting.model.mapObj
import org.json.JSONArray
import org.json.JSONObject

/** One stroke as the canvas draws it (`nativeInkStrokesForDraw`): world points `[x, y, …]`. */
class DrawStroke(val id: Int, val tool: Int, val width: Float, val argb: Int, val atMs: Long?, val pts: FloatArray) {
    val highlighter get() = tool == NoteTool.HIGHLIGHTER.core

    /** The stroke's world box, grown by half its width. */
    val bounds: WorldRect get() {
        var x0 = Double.MAX_VALUE; var y0 = Double.MAX_VALUE; var x1 = -Double.MAX_VALUE; var y1 = -Double.MAX_VALUE
        var i = 0
        while (i + 1 < pts.size) {
            x0 = minOf(x0, pts[i].toDouble()); x1 = maxOf(x1, pts[i].toDouble())
            y0 = minOf(y0, pts[i + 1].toDouble()); y1 = maxOf(y1, pts[i + 1].toDouble())
            i += 2
        }
        val m = width / 2.0
        return WorldRect(x0 - m, y0 - m, x1 + m, y1 + m)
    }

    companion object {
        fun from(o: JSONObject): DrawStroke? {
            val pts = o.arr("pts") ?: return null
            if (pts.length() < 2) return null
            val c = o.arr("color")
            val argb = if (c != null && c.length() == 4) {
                (c.optInt(3) shl 24) or (c.optInt(0) shl 16) or (c.optInt(1) shl 8) or c.optInt(2)
            } else {
                0xFF000000.toInt()
            }
            return DrawStroke(
                id = o.int("id") ?: return null,
                tool = o.int("tool") ?: 0,
                width = (o.opt("width") as? Number)?.toFloat() ?: 3f,
                argb = argb,
                atMs = o.long("atMs"),
                pts = FloatArray(pts.length()) { (pts.opt(it) as? Number)?.toFloat() ?: 0f },
            )
        }
    }
}

/** `nativeInkStrokesForDraw`: the strokes in view, the ink's bounds and the history state. */
data class InkView(val strokes: List<DrawStroke>, val bounds: WorldRect?, val canUndo: Boolean, val canRedo: Boolean) {
    companion object {
        val EMPTY = InkView(emptyList(), null, canUndo = false, canRedo = false)

        fun from(o: JSONObject): InkView = InkView(
            strokes = o.arr("strokes").mapObj(DrawStroke::from),
            bounds = rect(o.arr("bounds")),
            canUndo = o.bool("canUndo") ?: false,
            canRedo = o.bool("canRedo") ?: false,
        )

        /** `[x0, y0, x1, y1]` from the core, or null. */
        fun rect(a: JSONArray?): WorldRect? =
            WorldRect.of(a?.let { arr -> (0 until arr.length()).mapNotNull { (arr.opt(it) as? Number)?.toDouble() } })
    }
}

/**
 * F17-FR3 [Recommendation]: a mark draws a small ◆ beside the stroke written last before it:
 * just right of that stroke's top-right corner. Marks before any stroke have none.
 */
object MarkGlyphs {
    const val GAP = 8.0

    fun place(strokes: List<DrawStroke>, marksAtMs: List<Long>): List<Pair<Double, Double>> = marksAtMs.mapNotNull { at ->
        strokes.filter { it.atMs != null && it.atMs <= at }.maxByOrNull { it.atMs!! }?.bounds?.let { b -> (b.right + GAP) to b.top }
    }.distinct()
}

/**
 * F17-FR7 / F13-FR3: during playback, strokes written up to the position are drawn normally
 * and later ones muted; untimed strokes (written outside the recording) stay as they are. The
 * canvas redraws for it only when the muted set changes, and at most once a second (NFR2).
 */
object Emphasis {
    fun muted(s: DrawStroke, positionMs: Long?): Boolean = positionMs != null && s.atMs != null && s.atMs > positionMs

    fun changed(strokes: List<DrawStroke>, before: Long?, after: Long?): Boolean =
        strokes.any { muted(it, before) != muted(it, after) }
}
