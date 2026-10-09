package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** S15: Bluetooth profile proxies never leak, even when one arrives after close. */
class ProxySlotsTest {
    private val log = mutableListOf<String>()
    private val slots = ProxySlots<String>({ profile, p -> log += "close $profile:$p" }) { log += "change" }

    @Test fun proxiesAreKeptUntilCloseThenClosed() {
        slots.connected(1, "headset"); slots.connected(2, "a2dp")
        assertEquals("headset", slots[1])
        slots.close()
        assertEquals(listOf("change", "change", "close 1:headset", "close 2:a2dp"), log)
        assertNull(slots[1])
    }

    @Test fun aProxyArrivingAfterCloseIsClosedAtOnceAndNotReported() {
        slots.close()
        slots.connected(1, "late")
        slots.disconnected(1)
        assertEquals(listOf("close 1:late"), log)
        assertNull(slots[1])
    }

    @Test fun aReplacedOrDisconnectedProxyIsClosedOnce() { // F2-FR2: no leaked binding
        slots.connected(1, "a"); slots.connected(1, "b")
        slots.disconnected(1)
        assertEquals(listOf("change", "close 1:a", "change", "close 1:b", "change"), log)
        assertNull(slots[1])
        slots.disconnected(1) // nothing held: nothing to close or report
        slots.close(); slots.close()
        assertEquals(5, log.size) // nothing more to close
    }

    @Test fun everyProxyOpenedIsClosedExactlyOnceOnEveryPath() {
        slots.connected(1, "headset")
        slots.connected(2, "a2dp")
        slots.disconnected(2)
        slots.close()
        slots.connected(2, "a2dp-late") // bound after the session ended
        slots.disconnected(1)
        val closes = log.filter { it.startsWith("close") }
        assertEquals(listOf("close 2:a2dp", "close 1:headset", "close 2:a2dp-late"), closes)
    }
}
