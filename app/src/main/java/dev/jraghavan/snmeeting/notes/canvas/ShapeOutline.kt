// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/ShapeOutline.kt` (MIT
// License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes.canvas

import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/** A world point. */
data class Pt(val x: Double, val y: Double)

/**
 * Shape outlines as polylines in world units: what the painter strokes and fills for the
 * hand-drawn dash (F21-FR8). Box outlines are in the item's unrotated frame; the painter
 * applies the rotation.
 */
object ShapeOutline {
    private const val ELLIPSE_SEGMENTS = 64
    private const val WOBBLE_STEP = 16.0
    private const val WOBBLE_WAVELENGTH = 60.0
    private const val WOBBLE_SECOND_WAVELENGTH = 23.0
    private const val WOBBLE_MIX = 0.65

    /** A box item's closed outline (its first point is not repeated at the end). */
    fun of(item: DrawItem): List<Pt> = if (item.type == "ellipse") ellipse(item) else rectangle(item)

    private fun rectangle(e: DrawItem): List<Pt> =
        listOf(Pt(e.x, e.y), Pt(e.x + e.width, e.y), Pt(e.x + e.width, e.y + e.height), Pt(e.x, e.y + e.height))

    private fun ellipse(e: DrawItem): List<Pt> {
        val cx = e.x + e.width / 2
        val cy = e.y + e.height / 2
        return (0 until ELLIPSE_SEGMENTS).map { i ->
            val angle = 2 * PI * i / ELLIPSE_SEGMENTS
            Pt(cx + e.width / 2 * cos(angle), cy + e.height / 2 * sin(angle))
        }
    }

    /**
     * The hand-drawn look: [points] subdivided and nudged sideways by up to [amplitude],
     * smoothly, following a wobble seeded by [seed] (the item's id), so a shape wobbles the same
     * way on every redraw. A [closed] outline is walked back to its first point.
     */
    fun handDrawn(points: List<Pt>, closed: Boolean, seed: Int, amplitude: Double): List<Pt> {
        if (points.size < 2) return points
        val path = if (closed) points + points.first() else points
        val phase1 = (seed and 0xFFFF) / 65_535.0 * 2 * PI
        val phase2 = (seed ushr 16) / 65_535.0 * 2 * PI
        val result = mutableListOf<Pt>()
        var travelled = 0.0
        var normal = Pt(0.0, 0.0)
        for (i in 0 until path.size - 1) {
            val a = path[i]
            val b = path[i + 1]
            val length = hypot(b.x - a.x, b.y - a.y)
            normal = if (length == 0.0) normal else Pt(-(b.y - a.y) / length, (b.x - a.x) / length)
            val steps = ceil(length / WOBBLE_STEP).toInt().coerceAtLeast(1)
            for (step in 0 until steps) {
                val t = step.toDouble() / steps
                val offset = amplitude * wobble(travelled + length * t, phase1, phase2)
                result += Pt(a.x + (b.x - a.x) * t + normal.x * offset, a.y + (b.y - a.y) * t + normal.y * offset)
            }
            travelled += length
        }
        val last = path.last()
        val offset = amplitude * wobble(travelled, phase1, phase2)
        result += Pt(last.x + normal.x * offset, last.y + normal.y * offset)
        return result
    }

    /** The line a ramp fill runs along, x0, y0, x1, y1: straight down the shape. */
    fun rampLine(edge: Float, otherEdge: Float): FloatArray = floatArrayOf(0f, min(edge, otherEdge), 0f, max(edge, otherEdge))

    private fun wobble(distance: Double, phase1: Double, phase2: Double): Double =
        WOBBLE_MIX * sin(distance / WOBBLE_WAVELENGTH + phase1) +
            (1 - WOBBLE_MIX) * sin(distance / WOBBLE_SECOND_WAVELENGTH + phase2)
}
