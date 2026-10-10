package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.min

/** F20-FR7: one tag, laid out by the core (the largest circle's radius is 1). */
data class TagNode(val name: String, val count: Int, val x: Double, val y: Double, val r: Double)

/** Two tags used together: indexes into [TagGraph.nodes]. */
data class TagEdge(val a: Int, val b: Int, val shared: Int, val weak: Boolean)

/** `nativeTagGraph`'s graph. [bounds] is `[x0, y0, x1, y1]` around every circle. */
data class TagGraph(val nodes: List<TagNode>, val edges: List<TagEdge>, val meetings: Int, val bounds: List<Double>) {
    fun index(name: String): Int? = nodes.indexOfFirst { it.name == name }.takeIf { it >= 0 }

    companion object {
        val EMPTY = TagGraph(emptyList(), emptyList(), 0, listOf(0.0, 0.0, 0.0, 0.0))

        /** A reply as far as drawing goes: `laidOut` (whether the core laid it out just now)
         *  doesn't change the picture, so it doesn't cause a redraw (NFR2). */
        fun drawKey(json: String): String = json.replace("\"laidOut\":true", "\"laidOut\":false")

        /** `{range, graph: {...}, laidOut}`. */
        fun from(o: JSONObject): TagGraph? {
            val g = o.obj("graph") ?: return null
            val b = g.arr("bounds")
            return TagGraph(
                nodes = g.arr("nodes").mapObj { n ->
                    TagNode(n.str("name") ?: return@mapObj null, n.int("count") ?: 0,
                        n.optDouble("x", 0.0), n.optDouble("y", 0.0), n.optDouble("r", 1.0))
                },
                edges = g.arr("edges").mapObj { e ->
                    TagEdge(e.int("a") ?: return@mapObj null, e.int("b") ?: return@mapObj null, e.int("shared") ?: 1, e.bool("weak") ?: false)
                },
                meetings = g.int("meetings") ?: 0,
                bounds = if (b != null && b.length() == 4) (0 until 4).map { b.optDouble(it, 0.0) } else listOf(0.0, 0.0, 0.0, 0.0),
            )
        }
    }
}

/** World → screen: `screen = world * scale + offset`. */
data class Viewport(val scale: Double, val ox: Double, val oy: Double) {
    fun x(wx: Double) = wx * scale + ox
    fun y(wy: Double) = wy * scale + oy
    fun worldX(sx: Double) = (sx - ox) / scale
    fun worldY(sy: Double) = (sy - oy) / scale
}

/**
 * F20-FR7 / FR9: the arithmetic of drawing the core's still layout: Fit, zoom and pan,
 * circle sizes clamped to the view (≥ 48 dp across, the largest ≤ a quarter of the width),
 * which edges show, and which circle a tap is on. The view only draws what this says.
 */
object TagGraphMath {
    const val MIN_ZOOM = 0.25
    const val MAX_ZOOM = 8.0
    /** ＋ / − change the zoom by this much. */
    const val STEP = 1.5

    /** The whole graph in a [w] × [h] view with [pad] px around it; the largest circle (world
     *  radius 1) no wider than a quarter of the view. */
    fun fit(g: TagGraph, w: Int, h: Int, pad: Int): Viewport {
        val (x0, y0, x1, y1) = g.bounds
        val bw = (x1 - x0).coerceAtLeast(1e-6)
        val bh = (y1 - y0).coerceAtLeast(1e-6)
        val room = maxOf(1, w - 2 * pad).toDouble() to maxOf(1, h - 2 * pad).toDouble()
        val s = min(min(room.first / bw, room.second / bh), w / 8.0)
        return Viewport(s, w / 2.0 - (x0 + x1) / 2 * s, h / 2.0 - (y0 + y1) / 2 * s)
    }

    /** A circle's radius on screen: never under [minPx] (tappable), never over [maxPx]. */
    fun radiusPx(r: Double, vp: Viewport, minPx: Double, maxPx: Double): Double =
        (r * vp.scale).coerceIn(minPx, maxOf(minPx, maxPx))

    /** Zoom by [factor] about the screen point ([cx], [cy]), within [MIN_ZOOM]..[MAX_ZOOM] of [fit]. */
    fun zoom(vp: Viewport, factor: Double, cx: Double, cy: Double, fit: Viewport): Viewport {
        val s = (vp.scale * factor).coerceIn(fit.scale * MIN_ZOOM, fit.scale * MAX_ZOOM)
        val k = s / vp.scale
        return Viewport(s, cx - (cx - vp.ox) * k, cy - (cy - vp.oy) * k)
    }

    fun pan(vp: Viewport, dx: Double, dy: Double) = Viewport(vp.scale, vp.ox + dx, vp.oy + dy)

    /** OQ30: weak edges hide unless one of their tags is selected. */
    fun visibleEdges(g: TagGraph, selected: Int?): List<TagEdge> =
        g.edges.filter { !it.weak || it.a == selected || it.b == selected }

    /** An edge's stroke grows with the recordings its two tags share. */
    fun strokePx(shared: Int, unit: Double): Double = unit * (1.0 + 1.5 * ln(shared.coerceAtLeast(1).toDouble()))

    /** The circle under a tap, the nearest when circles are close; null on empty paper. */
    fun hit(g: TagGraph, vp: Viewport, x: Double, y: Double, minPx: Double, maxPx: Double): Int? =
        g.nodes.indices
            .map { i -> val n = g.nodes[i]; i to hypot(vp.x(n.x) - x, vp.y(n.y) - y) / radiusPx(n.r, vp, minPx, maxPx) }
            .filter { it.second <= 1.0 }
            .minByOrNull { it.second }?.first

    /** "#gpu 5" inside the circle when it fits, else beside it. */
    fun label(n: TagNode): String = "${Tags.shown(n.name)}  ${n.count}"
    fun fitsInside(textWidth: Float, radiusPx: Double): Boolean = textWidth <= radiusPx * 1.7

    /** F20-FR7's legend under the graph. */
    const val LEGEND = "Bigger circle = more recordings  ·  lines join tags used together  ·  tap a tag"

    /** F20-FR7's filters. */
    enum class Range(val id: String, val label: String) { ALL("all", "All time"), RECENT("3m", "Last 3 months") }
}
