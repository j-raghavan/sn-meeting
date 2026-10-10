package dev.jraghavan.snmeeting.notes

/** A rectangle in screen pixels, right and bottom exclusive. */
data class PxRect(val left: Int, val top: Int, val right: Int, val bottom: Int) {
    val width get() = right - left
    val height get() = bottom - top
    val empty get() = width <= 0 || height <= 0

    fun clip(o: PxRect) = PxRect(maxOf(left, o.left), maxOf(top, o.top), minOf(right, o.right), minOf(bottom, o.bottom))

    /** The firmware's form, `[left, top, width, height]`. */
    fun ltwh() = intArrayOf(left, top, width, height)

    fun contains(x: Int, y: Int) = x in left until right && y in top until bottom
}

/**
 * F17-FR4: where the firmware pen must not ink. Firmware ink paints wherever the stylus touches
 * our window, so without these a tap on the tool bar, the tabs or the strip left a mark (owner,
 * 2026-10-09). Everything on the screen outside the canvas is disabled: the bands above, below
 * and beside it (the top bar, the transcript strip, the docked bar on either side), plus the
 * controls drawn over the canvas ([holes]: the zoom pill, the open minimap). Pure, JVM-tested.
 */
object InkAreas {
    fun disabled(screen: PxRect, canvas: PxRect, holes: List<PxRect> = emptyList()): List<PxRect> {
        val c = canvas.clip(screen)
        if (c.empty) return listOf(screen)
        val bands = listOf(
            PxRect(screen.left, screen.top, screen.right, c.top), // above: status bar, tabs, Home, timer
            PxRect(screen.left, c.top, c.left, c.bottom), // left of it: the bar docked left
            PxRect(c.right, c.top, screen.right, c.bottom), // right of it: the bar docked right
            PxRect(screen.left, c.bottom, screen.right, screen.bottom), // below: the strip, opened or not
        )
        return (bands + holes.map { it.clip(c) }).filterNot { it.empty }
    }

    /** "n=3 [0,0,1920,180] …", the log line's tail. */
    fun describe(rects: List<PxRect>): String =
        "n=${rects.size}" + rects.joinToString("") { r -> " [" + r.ltwh().joinToString(",") + "]" }
}
