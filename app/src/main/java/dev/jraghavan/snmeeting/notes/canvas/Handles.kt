// The handle geometry follows sn-canvas `android/app/src/main/java/com/sncanvas/canvas/
// CanvasCore.kt` (`cornerPoints`, `rotateHandlePoint`, `handleAt`) and `ShapeEdits.kt`
// (MIT License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.DrawEntry
import dev.jraghavan.snmeeting.notes.WorldRect
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.sin

/**
 * F21-FR5: the handles of one selected item or stroke, dragged with the pen: four corners and
 * a rotate handle above the top edge for a box (rotated with it), or the two ends of a line or
 * arrow. Pure world geometry, JVM-tested; the canvas draws them and sends the drag's end to
 * the core, which does the edit.
 */
object Handles {
    enum class Kind { CORNER, ROTATE, END }

    /** [id] is `tl`/`tr`/`bl`/`br` for a corner, `rotate`, or `start`/`end`. */
    data class Handle(val kind: Kind, val id: String, val x: Double, val y: Double)

    /** Screen gap between a box's top edge and its rotate handle (`ROTATE_HANDLE_OFFSET_PX`). */
    const val ROTATE_OFFSET_PX = 56.0
    /** How near the pen must land to take a handle, in screen pixels. */
    const val TOUCH_PX = 28.0

    private val CORNERS = listOf("tl", "tr", "bl", "br")

    /** The handles of [e] at [zoom]; none for a type this build doesn't edit. */
    fun of(e: DrawEntry, zoom: Double): List<Handle> = when (e) {
        is DrawEntry.Ink -> box(e.stroke.bounds, 0.0, zoom)
        is DrawEntry.Thing -> {
            val it = e.item
            val ends = it.ends
            when {
                !it.known -> emptyList()
                ends != null -> listOf(Handle(Kind.END, "start", ends[0], ends[1]), Handle(Kind.END, "end", ends[2], ends[3]))
                else -> box(WorldRect(it.x, it.y, it.x + it.width, it.y + it.height), it.rotation, zoom)
            }
        }
    }

    private fun box(b: WorldRect, rotation: Double, zoom: Double): List<Handle> {
        val cx = (b.left + b.right) / 2
        val cy = (b.top + b.bottom) / 2
        fun turn(x: Double, y: Double): Pair<Double, Double> {
            if (rotation == 0.0) return x to y
            val c = cos(rotation); val s = sin(rotation)
            return (cx + (x - cx) * c - (y - cy) * s) to (cy + (x - cx) * s + (y - cy) * c)
        }
        val pts = listOf(b.left to b.top, b.right to b.top, b.left to b.bottom, b.right to b.bottom)
        return pts.mapIndexed { i, (x, y) -> turn(x, y).let { (wx, wy) -> Handle(Kind.CORNER, CORNERS[i], wx, wy) } } +
            turn(cx, b.top - ROTATE_OFFSET_PX / zoom).let { (wx, wy) -> Handle(Kind.ROTATE, "rotate", wx, wy) }
    }

    /** The handle under a world point, rotate first (`handleAt`), within [TOUCH_PX] on screen. */
    fun at(handles: List<Handle>, x: Double, y: Double, zoom: Double): Handle? {
        val r = TOUCH_PX / zoom
        return handles.sortedBy { if (it.kind == Kind.ROTATE) 0 else 1 }.firstOrNull { hypot(it.x - x, it.y - y) <= r }
    }

    /**
     * The width a box takes when its [corner] is dragged to ([x], [y]), the opposite corner
     * fixed (`ShapeEdits.resizeRotatedCorner`): what the shell measures a text box's new
     * height at before the core resizes it.
     */
    fun resizedWidth(item: DrawItem, corner: String, x: Double, y: Double): Double {
        val opposite = CORNERS[3 - CORNERS.indexOf(corner).coerceAtLeast(0)]
        val fixed = item.corners()[CORNERS.indexOf(opposite)]
        val dx = x - fixed.x
        val dy = y - fixed.y
        val c = cos(-item.rotation); val s = sin(-item.rotation)
        return abs(dx * c - dy * s)
    }
}
