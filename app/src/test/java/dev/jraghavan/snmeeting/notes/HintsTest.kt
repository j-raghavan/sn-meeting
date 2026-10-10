package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.hypot

class HintsTest { // F17-FR14, ported from sn-canvas draw_hints.py
    @Test fun everyControlHasAHint() {
        val ids = Hints.ALL.map { it.id }
        assertEquals(listOf("pen", "highlighter", "eraser", "lasso", "undo", "listen", "mark", "stop", "handle", "help", "zoom", "map", "strip"), ids)
    }

    @Test fun theArrowStartsAtTheCaptionAndEndsOnTheControl() {
        val (pts, toward) = Hints.arrow(0.0 to 0.0, 100.0 to -40.0)
        assertEquals(49, pts.size)
        assertEquals(0.0, pts.first().first, 1e-9)
        assertEquals(100.0, pts.last().first, 1e-9)
        assertEquals(-40.0, pts.last().second, 1e-9)
        val heads = Hints.head(100.0 to -40.0, toward)
        assertEquals(2, heads.size)
        heads.forEach { (x, y) -> assertEquals(Hints.HEAD_DP, hypot(x - 100.0, y + 40.0), 1e-9) }
    }

    @Test fun captionsSitBesideTheirControls() {
        val left = Hints.place(Hints.ALL.first { it.id == "pen" }, 80.0, 500.0 to 100.0)
        assertTrue(left.right < 500.0 - 56.0)
        assertEquals(500.0 - 56.0 to 128.0, left.tail)
        val right = Hints.place(Hints.ALL.first { it.id == "zoom" }, 80.0, 50.0 to 50.0)
        assertTrue(right.left > 50.0 + 56.0)
        val above = Hints.place(Hints.ALL.first { it.id == "strip" }, 120.0, 300.0 to 900.0)
        assertTrue(above.bottom < 900.0)
        assertFalse(left.meets(right))
        assertTrue(left.meets(left.copy(left = left.left + 1)))
    }
}
