// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/FirmwarePen.kt` (MIT
// License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md).
package dev.jraghavan.snmeeting.notes.canvas

/**
 * DEVICE-UNVERIFIED (R22: the nib, sizes and tones need the Manta). A pen as the firmware's
 * live ink takes it (`SupernoteInk`'s pen transaction): nib [type], [size] and [color], in the
 * firmware's own codes. F21-FR7: the Pen's live ink is the **pressure nib** at the style's size,
 * in the nearest of the firmware's four tones; the stroke shows in its own grey at the next bake.
 */
data class FirmwarePen(val type: Int, val size: Int, val color: Int) {
    companion object {
        // Firmware nib and colour codes, as Supernote's own notes send them (sn-canvas).
        const val NEEDLE = 10
        const val INK = 16
        const val BLACK = 0
        const val DARK_GRAY = -101
        const val GRAY = -102
        const val LIGHT_GRAY = 254
        const val CANVAS_SIZE = 1000

        /** The pen Meetings has always set (F17): the needle, black. Set back when the page goes. */
        val NEEDLE_BLACK = FirmwarePen(NEEDLE, CANVAS_SIZE, BLACK)

        /** The firmware size for a style size [Recommendation; R22]: M is the needle's 1000. */
        fun sizeOf(s: SizeStyle): Int = when (s) {
            SizeStyle.S -> 600
            SizeStyle.M -> 1000
            SizeStyle.L -> 1400
            SizeStyle.XL -> 2400
        }

        /** The firmware tone nearest a colour's e-ink grey (0x00…0xB4 spread over four). */
        fun toneOf(c: StyleColor): Int {
            val level = StylePalette.einkGray(c) and 0xFF
            return when {
                level < 0x2D -> BLACK
                level < 0x5A -> DARK_GRAY
                level < 0x87 -> GRAY
                else -> LIGHT_GRAY
            }
        }

        /** F21-FR7: the live pen for [style]. */
        fun forStyle(style: ShapeStyle): FirmwarePen = FirmwarePen(INK, sizeOf(style.size), toneOf(style.color))
    }
}
