// The hand-drawn arrow and caption layout are ported from sn-canvas `scripts/draw_hints.py`
// (MIT License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md); the captions are
// set in Virgil, Excalidraw's hand-drawn font (SIL Open Font License 1.1, assets/fonts).
package dev.jraghavan.snmeeting.notes

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.sin

/**
 * F17-FR14: the onboarding hints, as sn-canvas draws them: a hand-written caption and a
 * hand-drawn arrow curving from it to the control it names. Pure geometry (dp in, dp out),
 * JVM-tested; [HintsOverlay] draws it.
 */
object Hints {
    const val LINE_DP = 23.0
    const val GAP_DP = 6.0
    const val HEAD_DP = 10.0
    val HEAD_ANGLE = 28.0 * PI / 180
    const val WOBBLE_DP = 0.9

    /** Where the caption sits relative to the control the arrow points at. */
    enum class Side { LEFT, RIGHT, ABOVE }

    /** One hint: its caption lines, its control's id, and where the caption goes. */
    data class Spec(val id: String, val lines: List<String>, val side: Side, val reach: Double = 56.0)

    /** The hints, one per control (F17-FR14's list, with F21-FR16's Select, Shapes ▸,
     *  Insert ▸, Style ●, ⋮ and "tap Eraser again to clear"), in the order they are drawn. */
    val ALL = listOf(
        Spec("select", listOf("Select, move", "and resize"), Side.LEFT),
        Spec("pen", listOf("Write with", "the pen"), Side.LEFT),
        Spec("highlighter", listOf("Highlight"), Side.LEFT),
        Spec("eraser", listOf("Erase (or flip the pen);", "tap again to clear"), Side.LEFT),
        Spec("lasso", listOf("Circle ink", "to move it"), Side.LEFT),
        Spec("shapes", listOf("Shapes and arrows", "that stay attached"), Side.LEFT),
        Spec("insert", listOf("Text, sticky notes,", "tables, images"), Side.LEFT),
        Spec("style", listOf("Colour, fill", "and size"), Side.LEFT),
        Spec("undo", listOf("Undo & redo"), Side.LEFT),
        Spec("listen", listOf("Tap your ink", "to hear it"), Side.LEFT),
        Spec("more", listOf("Canvases, PDF,", "clear, zoom"), Side.LEFT),
        Spec("mark", listOf("Mark this", "moment"), Side.LEFT),
        Spec("stop", listOf("Stop recording"), Side.LEFT),
        Spec("handle", listOf("Fold the tools", "away"), Side.LEFT),
        Spec("help", listOf("Show these", "hints again"), Side.LEFT),
        Spec("zoom", listOf("Zoom, and Fit", "to see it all"), Side.RIGHT),
        Spec("map", listOf("The whole page:", "tap to go there"), Side.RIGHT, 80.0),
        Spec("strip", listOf("What's being said;", "tap to open it"), Side.ABOVE, 40.0),
    )

    /** The caption's box (left, top, right, bottom) and the arrow's tail, for a caption
     *  [textW] × [lines] lines placed on [side] of a control whose nearest edge is [tip]. */
    data class Placed(val left: Double, val top: Double, val right: Double, val bottom: Double, val tail: Pair<Double, Double>, val tip: Pair<Double, Double>) {
        fun meets(o: Placed) = left < o.right && o.left < right && top < o.bottom && o.top < bottom
    }

    fun place(spec: Spec, textW: Double, tip: Pair<Double, Double>): Placed {
        val h = spec.lines.size * LINE_DP
        return when (spec.side) {
            Side.LEFT -> {
                val tailX = tip.first - spec.reach
                val tailY = tip.second + spec.reach / 2
                Placed(tailX - GAP_DP - textW, tailY - h / 2, tailX - GAP_DP, tailY + h / 2, tailX to tailY, tip)
            }
            Side.RIGHT -> {
                val tailX = tip.first + spec.reach
                val tailY = tip.second + spec.reach / 2
                Placed(tailX + GAP_DP, tailY - h / 2, tailX + GAP_DP + textW, tailY + h / 2, tailX to tailY, tip)
            }
            Side.ABOVE -> {
                val tailY = tip.second - spec.reach
                val left = tip.first - textW / 2
                Placed(left, tailY - h - GAP_DP, left + textW, tailY - GAP_DP, tip.first + textW / 4 to tailY, tip)
            }
        }
    }

    private fun cubic(p0: Pair<Double, Double>, p1: Pair<Double, Double>, p2: Pair<Double, Double>, p3: Pair<Double, Double>, t: Double): Pair<Double, Double> {
        val u = 1 - t
        fun c(a: Double, b: Double, cc: Double, d: Double) = u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * cc + t * t * t * d
        return c(p0.first, p1.first, p2.first, p3.first) to c(p0.second, p1.second, p2.second, p3.second)
    }

    /** A curve leaving the caption sideways and reaching its tip head-on, wobbling slightly as
     *  a hand would (draw_hints.py `arrow_points`); also the point the head is swept back to. */
    fun arrow(tail: Pair<Double, Double>, tip: Pair<Double, Double>, samples: Int = 48): Pair<List<Pair<Double, Double>>, Pair<Double, Double>> {
        val p1 = (tail.first + 0.75 * (tip.first - tail.first)) to tail.second
        val p2 = tip.first to (tip.second - 0.45 * (tip.second - tail.second))
        val points = (0..samples).map { i ->
            val t = i.toDouble() / samples
            val (x, y) = cubic(tail, p1, p2, tip, t)
            val (ax, ay) = cubic(tail, p1, p2, tip, max(t - 0.01, 0.0))
            val (bx, by) = cubic(tail, p1, p2, tip, minOf(t + 0.01, 1.0))
            val len = hypot(bx - ax, by - ay).takeIf { it > 0 } ?: 1.0
            val wobble = WOBBLE_DP * sin(PI * t) * sin(2.6 * PI * t + 0.4)
            (x - (by - ay) / len * wobble) to (y + (bx - ax) / len * wobble)
        }
        return points to p2
    }

    /** The two strokes of an open arrowhead at [tip], swept back toward [toward]. */
    fun head(tip: Pair<Double, Double>, toward: Pair<Double, Double>): List<Pair<Double, Double>> {
        val dx = tip.first - toward.first
        val dy = tip.second - toward.second
        val len = hypot(dx, dy).takeIf { it > 0 } ?: 1.0
        val bx = -dx / len
        val by = -dy / len
        return listOf(1, -1).map { sign ->
            val a = sign * HEAD_ANGLE
            (tip.first + HEAD_DP * (bx * cos(a) - by * sin(a))) to (tip.second + HEAD_DP * (bx * sin(a) + by * cos(a)))
        }
    }
}
