package dev.jraghavan.snmeeting

import android.content.res.Resources

/**
 * F23: every screen at the same physical size on every e-ink tablet (owner, 2026-10-10: "scale
 * it per device"). These tablets are all about 300 ppi, but some report a much higher density
 * (the Musnap Aura C, 10.3" like the Manta, reports 420), which makes every dp about 1.3×
 * larger there. Android's own density override is ignored on that platform (it sets each app's
 * density itself), so Meetings scales its own sizes: [Inkwell] and the few views that read the
 * density directly use [density], which is Android's times [factor]. 1 on the Manta.
 */
object DeviceScale {
    /** The density the app was designed at on the Manta. */
    const val REFERENCE_DPI = 320

    /**
     * The narrowest screen, in dp, that still gets the Manta's layout unscaled. A narrower one
     * (the 8.2" Viwoods AiPaper Mini: 720 dp at a reported 320 dpi) draws smaller, in proportion
     * to its width, as the Musnap does (708 dp, scaled by density to 0.762). 900 keeps the
     * Manta (1024 dp) at 1 and the Musnap at its density factor, with room to spare.
     */
    const val REFERENCE_WIDTH_DP = 900

    /** How much smaller to draw on a device that reports [densityDpi] and is [smallestWidthDp]
     *  wide: 1 on the Manta, never more. A width of 0 (unknown) is ignored. */
    fun factor(densityDpi: Int, smallestWidthDp: Int = 0): Float {
        val byDensity = if (densityDpi > REFERENCE_DPI) REFERENCE_DPI.toFloat() / densityDpi else 1f
        val byWidth = if (smallestWidthDp in 1 until REFERENCE_WIDTH_DP) smallestWidthDp.toFloat() / REFERENCE_WIDTH_DP else 1f
        return minOf(byDensity, byWidth)
    }

    fun factor(r: Resources): Float = factor(r.displayMetrics.densityDpi, r.configuration.smallestScreenWidthDp)

    /** Pixels per Meetings dp on this device. */
    fun density(r: Resources): Float = r.displayMetrics.density * factor(r)
}
