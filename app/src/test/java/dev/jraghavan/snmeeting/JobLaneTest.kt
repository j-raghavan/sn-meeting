package dev.jraghavan.snmeeting

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class JobLaneTest {
    @Test fun oneJobPerKeyAndListenersHearTheEnd() {
        val queued = mutableListOf<Runnable>()
        val lane = JobLane(Jobs({ queued += it }, { it.run() }))
        val ended = mutableListOf<String>()
        val off = lane.onFinished { ended += it }
        var got = ""
        assertTrue(lane.run("m1", { "{}" }) { got = it })
        assertTrue(lane.isRunning("m1"))
        assertFalse(lane.run("m1", { "{}" }) {})
        assertTrue(lane.run("test:transcribe", { "{}" }) {})
        queued.removeAt(0).run()
        assertEquals("{}", got)
        assertEquals(listOf("m1"), ended)
        assertFalse(lane.isRunning("m1"))
        off()
        queued.removeAt(0).run()
        assertEquals(listOf("m1"), ended)
    }
}
