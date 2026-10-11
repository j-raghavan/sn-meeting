// Pan, zoom, fit and the minimap's frame adapted from sn-canvas `CanvasCore.kt` (panBy, zoomTo,
// zoomToPercent) and `ViewTransforms.kt` (fitTransform, fitToView, minimapLayout); MIT License,
// Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes

/** A world-space rectangle (world units are screen px at 100 %). */
data class WorldRect(val left: Double, val top: Double, val right: Double, val bottom: Double) {
    val width get() = right - left
    val height get() = bottom - top
    fun union(o: WorldRect) = WorldRect(minOf(left, o.left), minOf(top, o.top), maxOf(right, o.right), maxOf(bottom, o.bottom))
    fun contains(x: Double, y: Double) = x in left..right && y in top..bottom

    companion object {
        /** `[x0, y0, x1, y1]` from the core, or null. */
        fun of(a: List<Double>?): WorldRect? = a?.takeIf { it.size == 4 }?.let { WorldRect(it[0], it[1], it[2], it[3]) }
    }
}

/**
 * The canvas's viewport (F13-FR1, F17-FR1): the world point at the view's top-left and the
 * zoom, so `screen = (world − origin) × zoom`. Zoom stays within 5 %–2000 %.
 */
data class Viewport(val x: Double, val y: Double, val zoom: Double) {
    fun screenX(wx: Double) = (wx - x) * zoom
    fun screenY(wy: Double) = (wy - y) * zoom
    fun worldX(sx: Double) = x + sx / zoom
    fun worldY(sy: Double) = y + sy / zoom

    /** The world a [w] × [h] view shows. */
    fun visible(w: Double, h: Double) = WorldRect(x, y, x + w / zoom, y + h / zoom)

    /** A finger drag of ([dx], [dy]) screen px moves the world with it. */
    fun panBy(dx: Double, dy: Double) = copy(x = x - dx / zoom, y = y - dy / zoom)

    /** Zoom by [factor] about the screen point ([sx], [sy]), which stays put. */
    fun zoomAbout(factor: Double, sx: Double, sy: Double): Viewport {
        if (!(factor > 0)) return this
        val z = (zoom * factor).coerceIn(MIN_ZOOM, MAX_ZOOM)
        val fx = worldX(sx)
        val fy = worldY(sy)
        return Viewport(fx - sx / z, fy - sy / z, z)
    }

    /** The zoom pill's presets: [percent] of actual size about the view's centre. */
    fun zoomToPercent(percent: Int, w: Double, h: Double) = zoomAbout(percent / 100.0 / zoom, w / 2, h / 2)

    /** The world point at the view's centre (what `view.json` keeps). */
    fun centre(w: Double, h: Double) = (x + w / 2 / zoom) to (y + h / 2 / zoom)

    companion object {
        const val MIN_ZOOM = 0.05
        const val MAX_ZOOM = 20.0
        /** The zoom pill (F17-FR1): Fit, then these. */
        val PRESETS = listOf(100, 50, 25)
        private const val MIN_SPAN = 100.0

        /** The viewport whose centre is ([cx], [cy]) at [zoom] in a [w] × [h] view. */
        fun centred(cx: Double, cy: Double, zoom: Double, w: Double, h: Double): Viewport {
            val z = zoom.coerceIn(MIN_ZOOM, MAX_ZOOM)
            return Viewport(cx - w / 2 / z, cy - h / 2 / z, z)
        }

        /**
         * [world] fitted into a [w] × [h] box with [pad] px round it, centred, the zoom within
         * [range]. A zero-size axis counts as [MIN_SPAN].
         */
        fun fit(world: WorldRect, w: Double, h: Double, pad: Double, range: ClosedFloatingPointRange<Double> = MIN_ZOOM..MAX_ZOOM): Viewport {
            val cw = world.width.takeIf { it > 0 } ?: MIN_SPAN
            val ch = world.height.takeIf { it > 0 } ?: MIN_SPAN
            val z = minOf((w - 2 * pad).coerceAtLeast(1.0) / cw, (h - 2 * pad).coerceAtLeast(1.0) / ch).coerceIn(range)
            val cx = (world.left + world.right) / 2
            val cy = (world.top + world.bottom) / 2
            return Viewport(cx - w / 2 / z, cy - h / 2 / z, z)
        }

        /** Fit (EC21): all ink in view, zoomed out as far as needed but never past 100 %. An
         *  empty canvas opens at the origin at 100 %. */
        fun fitInk(bounds: WorldRect?, w: Double, h: Double, pad: Double): Viewport =
            bounds?.let { fit(it, w, h, pad, MIN_ZOOM..1.0) } ?: Viewport(0.0, 0.0, 1.0)
    }
}

/**
 * F17-FR13's minimap card, after inkread's `MinimapController` (AGPL-3.0, same owner): a
 * rounded white card at the canvas's top corner beside the tool bar, so the top-left corner
 * where writing starts stays clear (owner, 2026-10-10: "only the minimap on the top right"); it holds a thumbnail of
 * all the ink with the part in view marked, and −/+ buttons under it. This one geometry is what
 * both the drawing and the touch handling read, so they cannot disagree (inkread's pattern).
 * All values are canvas px; [fit] maps world to thumbnail px.
 */
data class MinimapFrame(
    val left: Double,
    val top: Double,
    val width: Double,
    val height: Double,
    val fit: Viewport,
    /** The −/+ row under the thumbnail. */
    val buttonTop: Double,
    val buttonH: Double,
    /** The card's padding round the thumbnail and the row. */
    val pad: Double,
) {
    val split get() = left + width / 2
    val buttonBottom get() = buttonTop + buttonH
    val cardLeft get() = left - pad
    val cardTop get() = top - pad
    val cardRight get() = left + width + pad
    val cardBottom get() = buttonBottom + pad

    /** F17-FR13: whether a view point ([x], [y]) is on the thumbnail. */
    fun contains(x: Double, y: Double) = x in left..(left + width) && y in top..(top + height)

    /** Anywhere on the card: a touch there never reaches the canvas. */
    fun inCard(x: Double, y: Double) = x in cardLeft..cardRight && y in cardTop..cardBottom

    private fun inRow(y: Double) = y in buttonTop..buttonBottom

    /** The − button; [split] is its, so − and + never overlap. */
    fun inMinus(x: Double, y: Double) = inRow(y) && x in left..split

    fun inPlus(x: Double, y: Double) = inRow(y) && x > split && x <= left + width

    /** The world point under a view point on the minimap (clamped to the box). */
    fun worldAt(x: Double, y: Double): Pair<Double, Double> =
        fit.worldX(x.coerceIn(left, left + width) - left) to fit.worldY(y.coerceIn(top, top + height) - top)

    /** The part in view on the thumbnail (left, top, right, bottom), clamped to it. */
    fun window(view: Viewport, w: Double, h: Double): DoubleArray {
        val v = view.visible(w, h)
        fun cx(x: Double) = (left + fit.screenX(x)).coerceIn(left, left + width)
        fun cy(y: Double) = (top + fit.screenY(y)).coerceIn(top, top + height)
        return doubleArrayOf(cx(v.left), cy(v.top), cx(v.right), cy(v.bottom))
    }

    /** F17-FR13: a tap at ([x], [y]) moves the view there: that world point at its centre. */
    fun centreOn(x: Double, y: Double, view: Viewport, w: Double, h: Double): Viewport {
        val (cx, cy) = worldAt(x, y)
        return Viewport.centred(cx, cy, view.zoom, w, h)
    }

    /** F17-FR13: dragging the view rectangle by ([dx], [dy]) minimap px pans the same world
     *  distance (this frame is held for the whole drag, as sn-canvas's MinimapLayout is). */
    fun dragBy(view: Viewport, dx: Double, dy: Double): Viewport = view.copy(x = view.x + dx / fit.zoom, y = view.y + dy / fit.zoom)

    /** Whether ([x], [y]) is on the view rectangle (a grab), rather than elsewhere (a jump). */
    fun onView(x: Double, y: Double, view: Viewport, w: Double, h: Double): Boolean {
        val (wx, wy) = worldAt(x, y)
        return contains(x, y) && view.visible(w, h).contains(wx, wy)
    }

    /** The − and + buttons: zoom about the view's centre. */
    fun zoomed(view: Viewport, zoomIn: Boolean, w: Double, h: Double): Viewport =
        view.zoomAbout(if (zoomIn) ZOOM_STEP else 1 / ZOOM_STEP, w / 2, h / 2)

    companion object {
        /** About a fifth of the canvas wide, never under 160 dp (owner, 2026-10-10: a quarter
         *  was oversized). */
        const val MIN_WIDTH_DP = 160.0
        const val WIDTH_FRACTION = 5.0
        const val MARGIN_DP = 12.0
        const val PAD_DP = 6.0
        const val INSET_DP = 8.0
        const val BUTTON_DP = 48.0
        const val GAP_DP = 6.0
        const val ZOOM_STEP = 1.25
        const val LINGER_MS = 3000L

        /**
         * The card for a [w] × [h] canvas at [density] px per dp, its top at [top] px (below the
         * zoom pill when they share a corner). The thumbnail keeps the canvas's shape and fits
         * the ink (or, with none, the view), so it changes only when the ink does.
         */
        fun of(ink: WorldRect?, view: Viewport, w: Double, h: Double, barOnLeft: Boolean, density: Double = 1.0, top: Double = MARGIN_DP * density): MinimapFrame? {
            if (w <= 0 || h <= 0) return null
            val margin = MARGIN_DP * density
            val pad = PAD_DP * density
            val tw = maxOf(w / WIDTH_FRACTION, MIN_WIDTH_DP * density).coerceAtMost(w - 2 * margin - 2 * pad).coerceAtLeast(1.0)
            val th = tw * h / w
            val left = if (barOnLeft) margin + pad else w - margin - pad - tw
            val world = ink ?: view.visible(w, h)
            val fit = Viewport.fit(world, tw, th, INSET_DP * density, 0.0..Double.MAX_VALUE)
            val t = top + pad
            return MinimapFrame(left, t, tw, th, fit, t + th + GAP_DP * density, BUTTON_DP * density, pad)
        }
    }
}
