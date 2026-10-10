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

    /** What the button says to accessibility services: "Back to Home". */
    fun label(to: String): String = "Back to " + to.trim().lowercase().replaceFirstChar { it.uppercase() }

    /** The house icon's size (owner, 2026-10-09: "Just Home Icon, Pressable"; no border). */
    const val ICON_DP = 34

    /** The pressable area around the icon, never under the touch minimum. */
    const val BUTTON_DP = 56

    /** The touch area in px at [density], never under 48 dp. */
    fun touchPx(density: Float): Int = kotlin.math.ceil(MIN_TOUCH_DP * density).toInt()
}
