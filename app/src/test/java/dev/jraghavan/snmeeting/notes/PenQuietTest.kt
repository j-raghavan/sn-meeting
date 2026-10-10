package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PenQuietTest { // F17-FR11, R14
    @Test fun redrawsWaitForThePenAndCollapseByKind() {
        val q = PenQuiet()
        val log = mutableListOf<String>()
        q.redraw("timer") { log += "t0" }
        assertEquals(listOf("t0"), log) // pen up: at once
        q.down()
        assertTrue(q.penDown)
        q.redraw("timer") { log += "t1" }
        q.redraw("strip") { log += "s1" }
        q.redraw("timer") { log += "t2" } // only the latest of a kind
        assertEquals(2, q.pending)
        assertEquals(listOf("t0"), log)
        q.up()
        assertFalse(q.penDown)
        assertEquals(listOf("t0", "t2", "s1"), log)
        assertEquals(0, q.pending)
    }
}
