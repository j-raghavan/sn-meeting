package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Test

class ExitGuardTest {
    private val log = mutableListOf<String>()

    @Test fun releasesInReverseOnceEvenIfOneThrows() {
        val g = ExitGuard { log += it }
        g.hold("wakelock") { log += "wake" }
        g.hold("sco") { throw IllegalStateException() }
        g.hold("callback") { log += "cb" }
        g.releaseAll(); g.releaseAll()
        assertEquals(listOf("cb", "release sco failed: IllegalStateException", "wake"), log)
        assertEquals(emptyList<String>(), g.names)
    }

    @Test fun earlyReleaseIsNotRepeated() {
        val g = ExitGuard()
        g.hold("wakelock") { log += "wake" }
        g.hold("sco") { log += "sco" }
        g.release("sco"); g.release("sco"); g.release("absent")
        assertEquals(listOf("wakelock"), g.names)
        g.releaseAll()
        assertEquals(listOf("sco", "wake"), log)
    }
}
