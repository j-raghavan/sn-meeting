package dev.jraghavan.snmeeting.notes

import android.view.View
import dev.jraghavan.snmeeting.notes.canvas.FirmwarePen

/**
 * F23-FR4: a device's own fast pen ink, drawn over the notes canvas while the pen writes. Each
 * e-ink platform has its own calls, so each has its own implementation, chosen by what the
 * device offers, never by its brand or model:
 * - [SupernoteInk]: the Supernote's pen daemon (the Manta, unchanged);
 * - [IreaderInk]: the iReader platform's system hand-write (the Musnap Aura C);
 * - otherwise [SupernoteInk], which does nothing without its daemon, so the canvas draws the
 *   live stroke itself, as before.
 * The canvas still receives every pen event and keeps the strokes; the fast ink only shows them
 * until the canvas redraws ([InkBake]).
 */
interface PenInk {
    /** Claim the pen for this window; true when the fast ink is on (safe to repeat). */
    fun setup(reason: String = ""): Boolean
    /** F21-FR7: the live pen (nib, size, tone). */
    var pen: FirmwarePen
    /** Wipe the fast ink (the canvas has redrawn every stroke). */
    fun clearAll()
    /** Fast ink on for the Pen, off for every other tool and while a panel is open. */
    fun setWritable(enable: Boolean)
    /** Screen-pixel rectangles where the pen must never ink (controls, panels). */
    fun setDisabledAreas(rects: List<PxRect>, where: String = "")
    /** Release the pen when the page goes. */
    fun teardown()
    /** One full-panel refresh (clears e-ink ghosting). */
    fun fullRefresh()
    fun systemGestures(enabled: Boolean)
    /** Called when fast ink that wasn't ready at [setup] becomes ready: the canvas claims again. */
    var onReady: (() -> Unit)?

    companion object {
        /** The fast ink this device has, for the canvas [canvas]. */
        fun forDevice(canvas: View): PenInk {
            val supernote = SupernoteInk(canvas.context)
            if (supernote.present()) return supernote
            return IreaderInk.create(canvas) ?: supernote
        }
    }
}
