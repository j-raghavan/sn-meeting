package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** S12: a destroyed service's last core replies still run, and recovery waits for them. */
class LateDeliveryTest {
    @Test fun aReplyRunsHereOnceTheRecordingThreadHasQuit() {
        val ran = mutableListOf<String>()
        val posted = mutableListOf<Runnable>()
        postOrRun({ posted += it; true }) { ran += "posted" }
        assertEquals(1, posted.size)
        assertEquals(emptyList<String>(), ran)
        postOrRun({ false }) { ran += "here" } // the looper is quitting: post refuses
        assertEquals(listOf("here"), ran)
    }

    @Test fun runningClearsOnlyAfterTheQueuedCoreCalls() {
        val queue = ArrayDeque<() -> Unit>()
        val flag = RunningFlag()
        flag.created()
        queue.addLast { /* the recorder's finish, queued by stop() */ }
        flag.destroyed { queue.addLast(it) }
        assertTrue("recovery must not see the meeting as dead yet", flag.running)
        queue.removeFirst()()
        assertTrue(flag.running)
        queue.removeFirst()()
        assertFalse(flag.running)
    }
}
