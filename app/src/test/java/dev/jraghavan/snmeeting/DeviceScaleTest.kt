package dev.jraghavan.snmeeting

import dev.jraghavan.snmeeting.notes.ScreenColor
import dev.jraghavan.snmeeting.notes.canvas.StylePalette
import org.junit.Assert.assertEquals
import org.junit.Test

class DeviceScaleTest { // F23: the same physical size, and colour where the panel has it
    @Test fun aDenseReportingDeviceDrawsAtTheMantasPhysicalSize() {
        assertEquals(320f / 420f, DeviceScale.factor(420), 1e-4f) // the Musnap Aura C
        assertEquals(1f, DeviceScale.factor(320), 1e-4f)
        assertEquals(1f, DeviceScale.factor(300), 1e-4f) // at or under the reference: unchanged
        assertEquals(1f, DeviceScale.factor(0), 1e-4f)
    }

    @Test fun eachTabletKeepsOrGetsItsScale() {
        // The Manta (300 dpi, 1920 px = 1024 dp) and the Musnap (420 dpi, 1860 px = 708 dp)
        // are exactly as before; the Viwoods AiPaper Mini (320 dpi, 1440 px = 720 dp) draws
        // smaller in proportion to its width.
        assertEquals(1f, DeviceScale.factor(300, 1024), 0f)
        assertEquals(320f / 420f, DeviceScale.factor(420, 708), 0f)
        assertEquals(720f / 900f, DeviceScale.factor(320, 720), 1e-6f)
        assertEquals(1f, DeviceScale.factor(320, 900), 0f)
        assertEquals(1f, DeviceScale.factor(320, 0), 0f) // width unknown: density only
        // The Musnap keeps its density factor down to 686 dp wide.
        assertEquals(320f / 420f, DeviceScale.factor(420, 686), 0f)
    }

    @Test fun aColourFilterPanelDrawsInTrueColour() {
        assertEquals(StylePalette.TRUE_COLOR, ScreenColor.palette { if (it == "ro.sys.eink.idisplay.cfa") "1" else null })
        assertEquals(StylePalette.EINK, ScreenColor.palette { if (it == "ro.sys.eink.idisplay.cfa") "0" else null })
        assertEquals(StylePalette.EINK, ScreenColor.palette { null }) // the Manta: no such property
    }
}
