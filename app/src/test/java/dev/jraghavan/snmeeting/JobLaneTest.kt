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

    @Test fun cancellingAQueuedJobTakesItOffTheLane() { // N9
        val queued = mutableListOf<Runnable>()
        val lane = JobLane(Jobs({ queued += it }, { it.run() }))
        val ended = mutableListOf<String>()
        lane.onFinished { ended += it }
        val ran = mutableListOf<String>()
        lane.run("m1", { ran += "m1"; "{}" }) {}
        var done2 = false
        lane.run("m2", { ran += "m2"; "{}" }) { done2 = true }
        queued.removeAt(0).run() // m1 starts and ends; m2 waits behind it
        assertFalse("a started job is the core's to cancel", lane.cancelQueued("m1"))
        assertTrue(lane.cancelQueued("m2"))
        assertFalse(lane.isRunning("m2"))
        assertEquals(listOf("m1", "m2"), ended)
        queued.removeAt(0).run()
        assertEquals(listOf("m1"), ran) // m2 never ran
        assertFalse(done2)
        assertFalse(lane.cancelQueued("m2"))
        assertTrue("its key is free again", lane.run("m2", { "{}" }) {})
    }

    @Test fun aMeetingJobIsBusyButATestConnectionIsNot() { // F14-FR8
        val queued = mutableListOf<Runnable>()
        val lane = JobLane(Jobs({ queued += it }, { it.run() }))
        assertFalse(lane.hasMeetingJob())
        lane.run("test:summarise", { "{}" }) {}
        assertFalse("Test connection doesn't hold back an update", lane.hasMeetingJob())
        lane.run("20261008-0930", { "{}" }) {}
        assertTrue(lane.hasMeetingJob())
        queued.removeAt(0).run()
        queued.removeAt(0).run()
        assertFalse(lane.hasMeetingJob())
        lane.run("m2", { "{}" }) {}
        lane.cancelQueued("m2")
        assertFalse("a cancelled queued job is gone", lane.hasMeetingJob())
    }
}
