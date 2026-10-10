// The palette cases are sn-canvas's `StylePaletteTest.kt` and `CanvasStyleTest.kt` (MIT License,
// Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md), carried as vectors.
package dev.jraghavan.snmeeting.notes.canvas

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CanvasStyleTest { // F21-FR6 / F21-AC9: true colour for the PDF, a grey per colour on e-ink
    private fun channels(argb: Int) = listOf(argb shr 16 and 0xFF, argb shr 8 and 0xFF, argb and 0xFF)

    @Test fun trueColourKeepsTldrawsColours() {
        assertEquals(0xFFE03131.toInt(), StylePalette.TRUE_COLOR.stroke(StyleColor.RED))
        assertEquals(0xFF4465E9.toInt(), StylePalette.TRUE_COLOR.stroke(StyleColor.BLUE))
    }

    @Test fun eInkGivesTwelveDistinctNeutralGreys() {
        val greys = StyleColor.entries.map { StylePalette.EINK.stroke(it) }
        assertEquals(12, greys.toSet().size)
        greys.forEach { assertEquals(1, channels(it).toSet().size) }
        assertEquals(0xFF000000.toInt(), StylePalette.EINK.stroke(StyleColor.BLACK))
        assertEquals(0xFFB4B4B4.toInt(), StylePalette.EINK.stroke(StyleColor.YELLOW))
        val byLuminance = StyleColor.entries.sortedBy { StylePalette.luminance(it.rgb) }
        val levels = byLuminance.map { StylePalette.einkGray(it) and 0xFF }
        assertEquals(levels.sorted(), levels)
    }

    @Test fun fillsAreLighterTintsInBothPalettes() {
        for (palette in StylePalette.entries) {
            val stroke = channels(palette.stroke(StyleColor.BLUE)).sum()
            val pattern = channels(palette.patternLine(StyleColor.BLUE)).sum()
            val solid = channels(palette.solidFill(StyleColor.BLUE)).sum()
            val semi = channels(palette.semiFill(StyleColor.BLUE)).sum()
            assertTrue("$palette", stroke < pattern && pattern < solid && solid < semi)
            assertEquals(palette.solidFill(StyleColor.RED), palette.fillTint(FillStyle.GRADIENT, StyleColor.RED))
            assertEquals(palette.semiFill(StyleColor.RED), palette.fillTint(FillStyle.NONE, StyleColor.RED))
        }
        assertEquals(0x00406080, StylePalette.fadeToNothing(0xFF406080.toInt()))
    }

    @Test fun unknownValuesLeaveTheStyleAlone() {
        val s = ShapeStyle.DEFAULT
        assertEquals(s, s.with("color", "magenta"))
        assertEquals(s, s.with("shadow", "on"))
        assertEquals(s, s.with("opacity", "NaN"))
        val b = s.with("color", "blue").with("size", "l").with("fill", "pattern").with("dash", "dotted").with("opacity", "0.01")
        assertEquals(ShapeStyle(StyleColor.BLUE, 0.1, FillStyle.PATTERN, DashStyle.DOTTED, SizeStyle.L), b)
        assertEquals(b, ShapeStyle.from(JSONObject("""{"color":"blue","opacity":0.1,"fill":"pattern","dash":"dotted","size":"l"}""")))
        assertEquals(s, ShapeStyle.from(JSONObject("""{"color":7,"size":"huge","opacity":"x"}""")))
        assertEquals(s, ShapeStyle.from(null))
        assertEquals(255, s.alpha)
        assertEquals(1f, ShapeStyle(size = SizeStyle.S).strokeWidthPx(0.1), 0f)
        assertEquals(10f, ShapeStyle(size = SizeStyle.XL).strokeWidthPx(1.0), 0f)
    }

    @Test fun handDrawnOutlinesAreStableAndClosed() {
        val pts = listOf(Pt(0.0, 0.0), Pt(100.0, 0.0), Pt(100.0, 50.0))
        val a = ShapeOutline.handDrawn(pts, closed = true, seed = 42, amplitude = 2.0)
        assertEquals(a, ShapeOutline.handDrawn(pts, closed = true, seed = 42, amplitude = 2.0))
        assertTrue(a.size > pts.size)
        assertTrue(a.all { it.x in -3.0..103.0 && it.y in -3.0..53.0 })
        assertEquals(pts.take(1), ShapeOutline.handDrawn(pts.take(1), closed = false, seed = 1, amplitude = 2.0))
        assertEquals(listOf(0f, 2f, 0f, 9f), ShapeOutline.rampLine(9f, 2f).toList())
    }
}
