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
 * The minimap (sn-canvas `Minimap.kt`, `ViewTransforms.minimapLayout`): shown while the canvas
 * moves and for a while after, as one repaint (no fade on e-ink). It frames all ink and the
 * part in view, in a box at the canvas's bottom corner away from the tool bar.
 */
data class MinimapFrame(val left: Double, val top: Double, val width: Double, val height: Double, val fit: Viewport) {
    companion object {
        const val WIDTH_PX = 300.0
        const val MARGIN_PX = 24.0
        const val PADDING_PX = 12.0
        const val LINGER_MS = 3000L

        fun of(ink: WorldRect?, view: Viewport, w: Double, h: Double, barOnLeft: Boolean): MinimapFrame? {
            if (w <= 0 || h <= 0) return null
            val boxH = (WIDTH_PX * h / w).coerceIn(160.0, 420.0)
            val visible = view.visible(w, h)
            val world = ink?.union(visible) ?: visible
            val left = if (barOnLeft) w - MARGIN_PX - WIDTH_PX else MARGIN_PX
            val fit = Viewport.fit(world, WIDTH_PX, boxH, PADDING_PX, 0.0..Double.MAX_VALUE)
            return MinimapFrame(left, h - MARGIN_PX - boxH, WIDTH_PX, boxH, fit)
        }
    }
}
