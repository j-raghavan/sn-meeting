package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RecordingHubTest {
    private val queued = mutableListOf<() -> Unit>()
    private val hub = RecordingHub { queued += it }

    @Test fun observersGetTheCurrentStateThenTransitionsOnTheMainThread() {
        val seen = mutableListOf<RecState>()
        val stop = hub.observe { seen += it }
        assertEquals(listOf<RecState>(RecState.Idle(null, null)), seen)
        hub.publish(RecState.Starting("USB — x"))
        assertTrue(hub.state.busy)
        assertEquals(1, seen.size) // not until the main thread runs it
        queued.removeAt(0)()
        assertEquals(RecState.Starting("USB — x"), seen.last())
        stop()
        hub.publish(RecState.Idle("Saved", "m"))
        queued.removeAt(0)()
        assertEquals(2, seen.size)
        assertFalse(hub.state.busy)
    }
}
