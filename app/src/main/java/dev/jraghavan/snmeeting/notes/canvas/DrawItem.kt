package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.model.arr
import dev.jraghavan.snmeeting.model.bool
import dev.jraghavan.snmeeting.model.int
import dev.jraghavan.snmeeting.model.long
import dev.jraghavan.snmeeting.model.obj
import dev.jraghavan.snmeeting.model.str
import dev.jraghavan.snmeeting.model.strings
import dev.jraghavan.snmeeting.notes.WorldRect
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.cos
import kotlin.math.sin

/** A table as the core resolved it: cells row by row and each row's height (F21-FR10). */
data class DrawTable(val rows: Int, val cols: Int, val cells: List<String>, val rowHeights: List<Double>)

/** An image's copy in the notes folder's `images/` and its size in pixels (F21-FR11). */
data class DrawImage(val file: String, val width: Int, val height: Int)

/** Where an item links to (F21-FR13); [followed] is false for a kind this build doesn't open. */
data class DrawLink(val kind: String, val target: String, val followed: Boolean)

/**
 * One item as `nativeCanvasForDraw` hands it (F21-FR1): its box and rotation, or for a line or
 * arrow its two ends already resolved (bound ends on their shapes' edges), its style, and its
 * text, table, image, group and link. An unknown type ([known] false) draws as a dashed box.
 */
data class DrawItem(
    val ref: String,
    val id: String,
    val type: String,
    val known: Boolean,
    val x: Double,
    val y: Double,
    val width: Double,
    val height: Double,
    val rotation: Double,
    /** `[startX, startY, endX, endY]` for a line or arrow, else null. */
    val ends: List<Double>?,
    val bound: Pair<Boolean, Boolean> = false to false,
    val style: ShapeStyle = ShapeStyle.DEFAULT,
    val text: String? = null,
    val table: DrawTable? = null,
    val image: DrawImage? = null,
    val groupId: String? = null,
    val link: DrawLink? = null,
    val recMs: Long? = null,
) {
    val hasEnds get() = ends != null

    /** A world point rotated by +[rotation] about the box's centre (unrotated frame → world). */
    fun toWorld(px: Double, py: Double): Pt = rotate(px, py, rotation)

    /** A world point into the unrotated frame. */
    fun toLocal(px: Double, py: Double): Pt = rotate(px, py, -rotation)

    private fun rotate(px: Double, py: Double, angle: Double): Pt {
        if (angle == 0.0) return Pt(px, py)
        val cx = x + width / 2
        val cy = y + height / 2
        val c = cos(angle)
        val s = sin(angle)
        return Pt(cx + (px - cx) * c - (py - cy) * s, cy + (px - cx) * s + (py - cy) * c)
    }

    /** The four corners in world space: top-left, top-right, bottom-left, bottom-right. */
    fun corners(): List<Pt> = listOf(toWorld(x, y), toWorld(x + width, y), toWorld(x, y + height), toWorld(x + width, y + height))

    /** Its world bounds (rotated box, or the two ends), grown by half its stroke. */
    val bounds: WorldRect get() {
        val pts = ends?.let { listOf(Pt(it[0], it[1]), Pt(it[2], it[3])) } ?: corners()
        val m = style.size.strokeWidth / 2
        return WorldRect(pts.minOf { it.x } - m, pts.minOf { it.y } - m, pts.maxOf { it.x } + m, pts.maxOf { it.y } + m)
    }

    companion object {
        private fun d(o: JSONObject, k: String) = (o.opt(k) as? Number)?.toDouble() ?: 0.0

        private fun doubles(a: JSONArray?): List<Double> =
            if (a == null) emptyList() else (0 until a.length()).mapNotNull { (a.opt(it) as? Number)?.toDouble() }

        fun from(ref: String, o: JSONObject): DrawItem? {
            val id = o.str("id") ?: return null
            val ends = doubles(o.arr("ends")).takeIf { it.size == 4 }
            val b = o.arr("bound")
            return DrawItem(
                ref = ref,
                id = id,
                type = o.str("type") ?: "",
                known = o.bool("known") ?: false,
                x = d(o, "x"), y = d(o, "y"), width = d(o, "width"), height = d(o, "height"),
                rotation = d(o, "rotation"),
                ends = ends,
                bound = (b?.optBoolean(0) ?: false) to (b?.optBoolean(1) ?: false),
                style = ShapeStyle.from(o.obj("style")),
                text = o.str("text"),
                table = o.obj("table")?.let { t ->
                    val rows = t.int("rows") ?: return@let null
                    val cols = t.int("cols") ?: return@let null
                    val cells = t.arr("cells").strings()
                    val heights = doubles(t.arr("rowHeights"))
                    if (cells.size != rows * cols || heights.size != rows) null else DrawTable(rows, cols, cells, heights)
                },
                image = o.obj("image")?.let { i ->
                    val f = i.str("file") ?: return@let null
                    DrawImage(f, i.int("width") ?: 0, i.int("height") ?: 0)
                },
                groupId = o.str("groupId"),
                link = o.obj("link")?.let { l -> DrawLink(l.str("kind") ?: "", l.str("target") ?: "", l.bool("followed") ?: false) },
                recMs = o.long("recMs"),
            )
        }
    }
}
