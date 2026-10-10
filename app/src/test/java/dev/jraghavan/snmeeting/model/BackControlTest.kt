package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BackControlTest { // F17-FR1 (device finding 2026-10-09)
    @Test fun aLabelledChevronWithA48dpTarget() {
        assertEquals("HOME", BackControl.label("Home"))
        assertEquals("BACK", BackControl.label(" back "))
        assertEquals("‹", BackControl.CHEVRON)
        assertTrue(BackControl.CHEVRON_DP >= 24)
        assertEquals(90, BackControl.touchPx(1.875f)) // the Manta's density: 48 dp
        assertEquals(48, BackControl.touchPx(1f))
        assertTrue(BackControl.touchPx(2.5f) >= 120)
    }
}
