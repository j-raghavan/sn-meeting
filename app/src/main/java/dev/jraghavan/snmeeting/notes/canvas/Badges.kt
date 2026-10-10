// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasCore.kt`
// (`linkGlyphPoint`, `linkGlyphAt`; MIT License, Copyright (c) 2026 JRaghavan; notice in
// LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.DrawEntry
import kotlin.math.hypot
import kotlin.math.sqrt

/**
 * F21-FR13: where a linked item's or stroke's chain badge sits: just outside its top-right
 * corner (off an ellipse's own curve), a fixed size on screen so it is as tappable zoomed out as
 * in; a line or arrow carries it at its end. JVM-tested.
 */
object Badges {
    /** Screen gap between the corner and the badge (`LINK_GLYPH_OFFSET_PX`). */
    const val OFFSET_PX = 34.0
    /** The badge's radius on screen, in dp (F21-FR13: 48 dp across). */
    const val RADIUS_DP = 24.0
    private val DIAGONAL = sqrt(2.0) / 2

    fun linked(e: DrawEntry): Boolean = when (e) {
        is DrawEntry.Ink -> e.stroke.linked
        is DrawEntry.Thing -> e.item.link != null
    }

    /** The badge's world point for [e] at [zoom]. */
    fun point(e: DrawEntry, zoom: Double): Pt {
        val gap = OFFSET_PX / zoom
        return when (e) {
            is DrawEntry.Ink -> e.stroke.bounds.let { Pt(it.right + gap, it.top - gap) }
            is DrawEntry.Thing -> {
                val it = e.item
                val ends = it.ends
                if (ends != null) {
                    Pt(ends[2], ends[3])
                } else {
                    val reach = if (it.type == "ellipse") DIAGONAL else 1.0
                    it.toWorld(it.x + it.width / 2 * (1 + reach) + gap, it.y + it.height / 2 * (1 - reach) - gap)
                }
            }
        }
    }

    /** The topmost linked entry whose badge is within [radiusWorld] of a world point. */
    fun at(entries: List<DrawEntry>, x: Double, y: Double, zoom: Double, radiusWorld: Double): DrawEntry? =
        entries.lastOrNull { linked(it) && point(it, zoom).let { p -> hypot(p.x - x, p.y - y) <= radiusWorld } }
}
