package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.canvas.StylePalette

/**
 * F23-FR5: whether this e-ink panel shows colour, found from what the device's own framework
 * says about its panel, never from its brand or model. Android itself reports no colour modes
 * on these panels, so the platform's property is the only sign: the iReader platform (the
 * Musnap Aura C, a Kaleido colour panel) sets `ro.sys.eink.idisplay.cfa=1` (colour filter
 * array). A colour panel draws styles in their true colours ([StylePalette.TRUE_COLOR], as the
 * PDF does); every other panel keeps a grey of its own per colour ([StylePalette.EINK]).
 */
object ScreenColor {
    /** System properties that mean "this panel has a colour filter" when they read "1". */
    val COLOR_PANEL_PROPERTIES = listOf("ro.sys.eink.idisplay.cfa")

    /** Pure: the palette for a device whose properties read as [prop] says. */
    fun palette(prop: (String) -> String?): StylePalette =
        if (COLOR_PANEL_PROPERTIES.any { prop(it)?.trim() == "1" }) StylePalette.TRUE_COLOR else StylePalette.EINK

    /** This device's palette (android.os.SystemProperties, by reflection; grey if unreadable). */
    fun palette(): StylePalette = palette(::systemProperty)

    private fun systemProperty(key: String): String? = try {
        Class.forName("android.os.SystemProperties").getMethod("get", String::class.java).invoke(null, key) as? String
    } catch (t: Throwable) {
        null
    }
}
