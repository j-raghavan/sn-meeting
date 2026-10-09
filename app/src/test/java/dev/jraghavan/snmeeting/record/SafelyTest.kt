package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The recorder thread's guard: a throw becomes an orderly end, never a crash. */
class SafelyTest {
    private val log = mutableListOf<String>()

    @Test fun aThrowIsLoggedAndEndsTheRecording() {
        val s = Safely({ m, t -> log += "$m ${t.javaClass.simpleName}" }) { log += "end" }
        s.run("devices") { throw IllegalStateException() }
        s.run("timer") { log += "ran" }
        assertEquals(listOf("devices failed IllegalStateException", "end", "ran"), log)
    }

    @Test fun anEndThatThrowsIsReportedNotRethrown() {
        val s = Safely({ m, _ -> log += m }) { throw IllegalStateException("end") }
        assertFalse(s.fail("uncaught", RuntimeException()))
        assertEquals(listOf("uncaught failed", "ending after a fault failed"), log)
    }

    @Test fun anOrderlyEndIsReported() {
        assertTrue(Safely({ _, _ -> }) {}.fail("uncaught", RuntimeException()))
    }
}
