package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DialogGateTest { // F17-FR14: nothing opens over the canvas while the pen is in use
    private var t = 10_000L
    private val gate = DialogGate({ t })
    private val shown = mutableListOf<String>()

    @Test fun anExplicitDialogOpensWithThePenUpAndIsDroppedMidStroke() { // F21-FR3, EC32, F21-AC5
        gate.pen(true)
        assertFalse(gate.explicit { shown += "clear" })
        gate.pen(false)
        t += 1_000; gate.tick()
        assertTrue("dropped, never queued", shown.isEmpty())
        assertEquals(0, gate.pending)
        assertTrue("no 1.5 s wait once the pen is up", gate.explicit { shown += "clear" })
        assertEquals(listOf("clear"), shown)
    }

    @Test fun anIdleCanvasShowsADialogAtOnce() {
        gate.show { shown += "a" }
        assertEquals(listOf("a"), shown)
    }

    @Test fun aDialogAskedForWhileWritingWaitsForTheCanvasToRest() {
        gate.pen(true)
        gate.show { shown += "notice" }
        gate.show { shown += "error" }
        assertTrue(shown.isEmpty())
        t += 5_000; gate.tick()
        assertTrue("the pen is still down", shown.isEmpty())
        gate.pen(false)
        t += 1_000; gate.tick()
        assertTrue("not idle long enough", shown.isEmpty())
        assertFalse(gate.idle())
        t += 500; gate.tick()
        assertEquals(listOf("notice", "error"), shown)
        assertEquals(0, gate.pending)
    }
}
