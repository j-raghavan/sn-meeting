package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InkAreasTest { // F17-FR4: the firmware pen never inks over the controls
    private val screen = PxRect(0, 0, 1920, 2560)
    private val top = 200 // status bar and the top bar
    private val strip = 260 // the closed strip; opened it takes half the column
    private val bar = 120 // the docked bar (64 dp at 1.875) and its rule
    private val collapsedBar = 120

    /** The canvas the page lays out: the column less the strip, beside the bar. */
    private fun canvas(barW: Int, left: Boolean, stripH: Int) =
        PxRect(if (left) barW else 0, top, if (left) 1920 else 1920 - barW, 2560 - stripH)

    private fun covered(rs: List<PxRect>, x: Int, y: Int) = rs.any { it.contains(x, y) }

    private fun assertOnlyCanvasWritable(c: PxRect, rs: List<PxRect>) {
        for (x in 0 until 1920 step 20) for (y in 0 until 2560 step 20) {
            assertEquals("($x,$y)", !c.contains(x, y), covered(rs, x, y))
        }
    }

    @Test fun barOnTheRightIsDisabledWholeTopToBottom() {
        val c = canvas(bar, left = false, stripH = strip)
        val rs = InkAreas.disabled(screen, c)
        assertTrue(rs.contains(PxRect(1920 - bar, top, 1920, 2560 - strip)))
        assertTrue(rs.contains(PxRect(0, 0, 1920, top)) && rs.contains(PxRect(0, 2560 - strip, 1920, 2560)))
        assertEquals(3, rs.size)
        assertOnlyCanvasWritable(c, rs)
    }

    @Test fun barOnTheLeft() {
        val c = canvas(bar, left = true, stripH = strip)
        val rs = InkAreas.disabled(screen, c)
        assertTrue(rs.contains(PxRect(0, top, bar, 2560 - strip)))
        assertOnlyCanvasWritable(c, rs)
    }

    @Test fun collapsedAndExpandedEachFollowTheCanvas() {
        for (w in listOf(bar, collapsedBar, 90)) assertOnlyCanvasWritable(canvas(w, false, strip), InkAreas.disabled(screen, canvas(w, false, strip)))
    }

    @Test fun stripOpenedOrHiddenByFocus() {
        val open = canvas(bar, false, stripH = (2560 - top) / 2)
        assertOnlyCanvasWritable(open, InkAreas.disabled(screen, open))
        val focus = canvas(bar, false, stripH = 0) // no strip: nothing below the canvas
        val rs = InkAreas.disabled(screen, focus)
        assertEquals(2, rs.size)
        assertOnlyCanvasWritable(focus, rs)
    }

    @Test fun theZoomPillAndMinimapAreHolesClippedToTheCanvas() {
        val c = canvas(bar, false, strip)
        val pill = PxRect(22, top + 22, 700, top + 110)
        val map = PxRect(1500, 2000, 1900, 2400) // runs under the bar: clipped to the canvas
        val rs = InkAreas.disabled(screen, c, listOf(pill, map))
        assertTrue(rs.contains(pill) && rs.contains(PxRect(1500, 2000, 1920 - bar, 2560 - strip)))
        assertTrue(covered(rs, 100, top + 60)); assertFalse(covered(rs, 100, top + 400))
    }

    @Test fun noCanvasDisablesTheWholeScreen() {
        assertEquals(listOf(screen), InkAreas.disabled(screen, PxRect(0, 0, 0, 0)))
    }

    @Test fun theLogLineGivesLeftTopWidthHeight() {
        assertEquals("n=2 [0,0,1920,200] [1800,200,120,2100]",
            InkAreas.describe(listOf(PxRect(0, 0, 1920, 200), PxRect(1800, 200, 1920, 2300))))
    }
}
