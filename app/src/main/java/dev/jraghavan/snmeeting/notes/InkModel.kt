package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.model.arr
import dev.jraghavan.snmeeting.model.bool
import dev.jraghavan.snmeeting.model.int
import dev.jraghavan.snmeeting.model.long
import dev.jraghavan.snmeeting.model.mapObj
import dev.jraghavan.snmeeting.model.obj
import dev.jraghavan.snmeeting.model.str
import dev.jraghavan.snmeeting.model.strings
import dev.jraghavan.snmeeting.notes.canvas.DrawItem
import dev.jraghavan.snmeeting.notes.canvas.ShapeStyle
import org.json.JSONArray
import org.json.JSONObject

/**
 * One stroke as the canvas draws it (`nativeInkStrokesForDraw`, `nativeCanvasForDraw`): world
 * points `[x, y, …]`, and (F21) its style ids, group and link from `items.json`.
 */
class DrawStroke(
    val id: Int,
    val tool: Int,
    val width: Float,
    val argb: Int,
    val atMs: Long?,
    val pts: FloatArray,
    val style: ShapeStyle? = null,
    val groupId: String? = null,
) {
    val ref get() = "s:$id"

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
                style = o.obj("style")?.let(ShapeStyle::from),
                groupId = o.str("groupId"),
            )
        }
    }
}

/** F21-FR1: one paint-ordered thing on the canvas: a stroke or an item. */
sealed interface DrawEntry {
    val ref: String

    data class Ink(val stroke: DrawStroke) : DrawEntry {
        override val ref get() = stroke.ref
    }

    data class Thing(val item: DrawItem) : DrawEntry {
        override val ref get() = item.ref
    }
}

/**
 * `nativeInkStrokesForDraw` / `nativeCanvasForDraw`: what is in view in paint order, the
 * content's bounds, the history state and (F21) the selection and whether items can be edited.
 */
data class InkView(
    val strokes: List<DrawStroke>,
    val bounds: WorldRect?,
    val canUndo: Boolean,
    val canRedo: Boolean,
    val entries: List<DrawEntry> = strokes.map { DrawEntry.Ink(it) },
    val selection: List<String> = emptyList(),
    val selectionBounds: WorldRect? = null,
    val itemsOk: Boolean = true,
    /** OQ34: a cleared canvas's backup can be restored. */
    val canRestore: Boolean = false,
    /** F21-FR6: the style new items and strokes take. */
    val defaultStyle: ShapeStyle = ShapeStyle.DEFAULT,
) {
    val items: List<DrawItem> get() = entries.mapNotNull { (it as? DrawEntry.Thing)?.item }

    companion object {
        val EMPTY = InkView(emptyList(), null, canUndo = false, canRedo = false)

        fun from(o: JSONObject): InkView {
            val entries = o.arr("entries")?.let { a ->
                (0 until a.length()).mapNotNull { i ->
                    val e = a.opt(i) as? JSONObject ?: return@mapNotNull null
                    val ref = e.str("ref") ?: return@mapNotNull null
                    e.obj("stroke")?.let(DrawStroke::from)?.let { DrawEntry.Ink(it) }
                        ?: e.obj("item")?.let { DrawItem.from(ref, it) }?.let { DrawEntry.Thing(it) }
                }
            }
            val strokes = entries?.mapNotNull { (it as? DrawEntry.Ink)?.stroke } ?: o.arr("strokes").mapObj(DrawStroke::from)
            return InkView(
                strokes = strokes,
                bounds = rect(o.arr("bounds")),
                canUndo = o.bool("canUndo") ?: false,
                canRedo = o.bool("canRedo") ?: false,
                entries = entries ?: strokes.map { DrawEntry.Ink(it) },
                selection = o.arr("selection").strings(),
                selectionBounds = rect(o.arr("selectionBounds")),
                itemsOk = o.bool("itemsOk") ?: true,
                canRestore = o.bool("canRestore") ?: false,
                defaultStyle = ShapeStyle.from(o.obj("defaultStyle")),
            )
        }

        /** `[x0, y0, x1, y1]` from the core, or null. */
        fun rect(a: JSONArray?): WorldRect? =
            WorldRect.of(a?.let { arr -> (0 until arr.length()).mapNotNull { (arr.opt(it) as? Number)?.toDouble() } })
    }
}

/** `nativeInkHit` (F17-FR7): the stroke under a Listen tap, and where playback starts. */
data class ListenHit(val strokeId: Int?, val atMs: Long?, val seekMs: Long?, val lineN: Int?) {
    companion object {
        fun from(o: JSONObject): ListenHit = ListenHit(o.int("strokeId"), o.long("atMs"), o.long("seekMs"), o.int("lineN"))
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
