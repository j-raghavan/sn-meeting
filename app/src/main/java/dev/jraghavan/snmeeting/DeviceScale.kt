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

    /** How much smaller to draw on a device that reports [densityDpi]: 1 at or under the reference. */
    fun factor(densityDpi: Int): Float = if (densityDpi > REFERENCE_DPI) REFERENCE_DPI.toFloat() / densityDpi else 1f

    /** Pixels per Meetings dp on this device. */
    fun density(r: Resources): Float = r.displayMetrics.let { it.density * factor(it.densityDpi) }
}
