// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasStyle.kt` and
// `StylePalette.kt` (MIT License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md).
// The colour values are tldraw's published defaults, used as data.
package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.model.str
import org.json.JSONObject
import kotlin.math.roundToInt

/** A style option stored by its id, such as "light-blue" or "dashed" (F21-FR6). */
interface StyleOption {
    val id: String
}

/** The option with [id], or [fallback] for an unknown or missing id. */
fun <E : StyleOption> List<E>.byId(id: String?, fallback: E): E = firstOrNull { it.id == id } ?: fallback

/** tldraw's 12 colours with their true RGB; the live view draws each as a grey of its own ([StylePalette.EINK]), or in colour on a colour panel ([StylePalette.screen]). */
enum class StyleColor(override val id: String, val rgb: Int, val label: String) : StyleOption {
    BLACK("black", 0x1D1D1D, "Black"),
    GREY("grey", 0x9FA8B2, "Grey"),
    LIGHT_VIOLET("light-violet", 0xE085F4, "Light violet"),
    VIOLET("violet", 0xAE3EC9, "Violet"),
    BLUE("blue", 0x4465E9, "Blue"),
    LIGHT_BLUE("light-blue", 0x4BA1F1, "Light blue"),
    YELLOW("yellow", 0xF1AC4B, "Yellow"),
    ORANGE("orange", 0xE16919, "Orange"),
    GREEN("green", 0x099268, "Green"),
    LIGHT_GREEN("light-green", 0x4CB05E, "Light green"),
    LIGHT_RED("light-red", 0xF87777, "Light red"),
    RED("red", 0xE03131, "Red"),
}

enum class FillStyle(override val id: String, val label: String, val ramps: Boolean = false) : StyleOption {
    NONE("none", "None"),
    SEMI("semi", "Semi"),
    SOLID("solid", "Solid"),
    PATTERN("pattern", "Hatched"),
    GRADIENT("gradient", "Gradient", ramps = true),
}

enum class DashStyle(override val id: String, val label: String) : StyleOption {
    /** The hand-drawn wobble; see [ShapeOutline.handDrawn]. */
    DRAW("draw", "Hand-drawn"),
    DASHED("dashed", "Dashed"),
    DOTTED("dotted", "Dotted"),
    SOLID("solid", "Solid"),

    /** No outline: an image's frame can be left off. New items never start without one. */
    NONE("none", "None"),
}

/** Stroke weights and text sizes in world units (tldraw's s/m/l/xl). */
enum class SizeStyle(override val id: String, val strokeWidth: Double, val fontSize: Double) : StyleOption {
    S("s", 2.0, 18.0),
    M("m", 3.5, 24.0),
    L("l", 5.0, 36.0),
    XL("xl", 10.0, 44.0),
}

/** An item's look: colour, opacity, fill, dash and size. Unknown ids read as the default's. */
data class ShapeStyle(
    val color: StyleColor = StyleColor.BLACK,
    val opacity: Double = 1.0,
    val fill: FillStyle = FillStyle.NONE,
    val dash: DashStyle = DashStyle.DRAW,
    val size: SizeStyle = SizeStyle.M,
) {
    /** Paint alpha (0..255) for this style's opacity. */
    val alpha: Int get() = (opacity.coerceIn(MIN_OPACITY, 1.0) * 255).roundToInt()

    /** The stroke width in screen pixels at [zoom]; never thinner than 1 px. */
    fun strokeWidthPx(zoom: Double): Float = (size.strokeWidth * zoom).toFloat().coerceAtLeast(MIN_STROKE_PX)

    /** This style with one property changed, as the style panel sends it; unknown input leaves it. */
    fun with(property: String, value: String): ShapeStyle = when (property) {
        "color" -> copy(color = StyleColor.entries.byId(value, color))
        "fill" -> copy(fill = FillStyle.entries.byId(value, fill))
        "dash" -> copy(dash = DashStyle.entries.byId(value, dash))
        "size" -> copy(size = SizeStyle.entries.byId(value, size))
        "opacity" -> copy(opacity = value.toDoubleOrNull()?.takeUnless { it.isNaN() }?.coerceIn(MIN_OPACITY, 1.0) ?: opacity)
        else -> this
    }

    /** The core's `style` object for this style. */
    fun toJson(): String =
        """{"color":"${color.id}","opacity":$opacity,"fill":"${fill.id}","dash":"${dash.id}","size":"${size.id}"}"""

    companion object {
        const val MIN_OPACITY = 0.1
        const val MIN_STROKE_PX = 1f

        val DEFAULT = ShapeStyle()

        /** The core's `style` object (unknown or missing values: the default's). */
        fun from(o: JSONObject?): ShapeStyle {
            if (o == null) return DEFAULT
            return ShapeStyle(
                color = StyleColor.entries.byId(o.str("color"), DEFAULT.color),
                opacity = (o.opt("opacity") as? Number)?.toDouble()?.takeIf { it.isFinite() }?.coerceIn(MIN_OPACITY, 1.0) ?: 1.0,
                fill = FillStyle.entries.byId(o.str("fill"), DEFAULT.fill),
                dash = DashStyle.entries.byId(o.str("dash"), DEFAULT.dash),
                size = SizeStyle.entries.byId(o.str("size"), DEFAULT.size),
            )
        }
    }
}

/**
 * ARGB values a painter uses. [TRUE_COLOR] keeps tldraw's colours, for the PDF (F21-FR14).
 * [EINK] gives each colour a grey of its own for the live view, spread evenly in the colours'
 * luminance order so hues of similar brightness never share a shade (F21-FR6). Fills are the
 * stroke tone moved toward white, so they read as tints of it.
 */
enum class StylePalette {
    TRUE_COLOR,
    EINK,
    ;

    /** The opaque stroke colour. */
    fun stroke(color: StyleColor): Int = OPAQUE or if (this == TRUE_COLOR) color.rgb else einkGray(color)

    fun semiFill(color: StyleColor): Int = mixWithWhite(stroke(color), SEMI_WHITENESS)

    fun solidFill(color: StyleColor): Int = mixWithWhite(stroke(color), SOLID_WHITENESS)

    /** The hatch lines of a pattern fill, drawn over [semiFill]. */
    fun patternLine(color: StyleColor): Int = mixWithWhite(stroke(color), PATTERN_WHITENESS)

    /** The flat colour a [fill] of [color] is painted in (one home for the shape fill and the note tint). */
    fun fillTint(fill: FillStyle, color: StyleColor): Int = when (fill) {
        FillStyle.SOLID, FillStyle.GRADIENT -> solidFill(color)
        FillStyle.NONE, FillStyle.SEMI, FillStyle.PATTERN -> semiFill(color)
    }

    companion object {
        /** F23-FR5: the palette the live view draws with on this device ([EINK] unless the
         *  panel shows colour; set once when the notes page opens, from ScreenColor). */
        @Volatile var screen: StylePalette = EINK

        private const val OPAQUE = 0xFF shl 24
        private const val NO_ALPHA = 0x00FFFFFF
        private const val DARKEST_GRAY = 0x00
        private const val LIGHTEST_GRAY = 0xB4
        private const val SEMI_WHITENESS = 0.85
        private const val SOLID_WHITENESS = 0.45
        private const val PATTERN_WHITENESS = 0.3

        private val einkGrays: Map<StyleColor, Int> =
            StyleColor.entries.sortedBy { luminance(it.rgb) }.withIndex().associate { (rank, color) ->
                val level = DARKEST_GRAY + rank * (LIGHTEST_GRAY - DARKEST_GRAY) / (StyleColor.entries.size - 1)
                color to (level shl 16 or (level shl 8) or level)
            }

        /** The e-ink grey (0xRRGGBB, R = G = B) the live view draws [color] with. */
        fun einkGray(color: StyleColor): Int = einkGrays.getValue(color)

        /** Relative luminance (Rec. 709) of a 0xRRGGBB colour, on a 0..255 scale. */
        fun luminance(rgb: Int): Double = 0.2126 * (rgb shr 16 and 0xFF) + 0.7152 * (rgb shr 8 and 0xFF) + 0.0722 * (rgb and 0xFF)

        /** [argb] with its alpha cleared: the far end of a gradient fill. */
        fun fadeToNothing(argb: Int): Int = argb and NO_ALPHA

        /** [argb] moved [whiteness] (0..1) of the way to white, alpha unchanged. */
        fun mixWithWhite(argb: Int, whiteness: Double): Int {
            fun channel(shift: Int): Int {
                val value = argb shr shift and 0xFF
                return (value + (0xFF - value) * whiteness).roundToInt() shl shift
            }
            return (argb and OPAQUE) or channel(16) or channel(8) or channel(0)
        }
    }
}
