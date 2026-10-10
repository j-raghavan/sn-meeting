package dev.jraghavan.snmeeting.model

/**
 * The screens' back control (device finding 2026-10-09: the bare "←" was too small to see or
 * hit): a bold chevron and a word, "‹ HOME" on the meeting page, "‹ BACK" or the screen it
 * returns to elsewhere, with a touch area of at least [MIN_TOUCH_DP] in both directions.
 */
object BackControl {
    const val MIN_TOUCH_DP = 48
    const val CHEVRON = "‹"
    /** The chevron's size: bold, at least 24 dp. */
    const val CHEVRON_DP = 28

    fun label(to: String): String = to.trim().uppercase()

    /** The touch area in px at [density], never under 48 dp. */
    fun touchPx(density: Float): Int = kotlin.math.ceil(MIN_TOUCH_DP * density).toInt()
}
