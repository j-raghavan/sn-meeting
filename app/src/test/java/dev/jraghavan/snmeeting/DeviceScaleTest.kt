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

    @Test fun aColourFilterPanelDrawsInTrueColour() {
        assertEquals(StylePalette.TRUE_COLOR, ScreenColor.palette { if (it == "ro.sys.eink.idisplay.cfa") "1" else null })
        assertEquals(StylePalette.EINK, ScreenColor.palette { if (it == "ro.sys.eink.idisplay.cfa") "0" else null })
        assertEquals(StylePalette.EINK, ScreenColor.palette { null }) // the Manta: no such property
    }
}
