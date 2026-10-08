package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class WakeLeaseTest {
    private val log = mutableListOf<String>()
    private val wake = object : WakePort {
        override fun acquire(timeoutMs: Long) { log += "acquire $timeoutMs" }
        override fun release() { log += "release" }
    }

    @Test fun heldWithATimeoutRenewedPerSegmentReleasedOnceByTheGuard() {
        val guard = ExitGuard()
        val lease = WakeLease(wake)
        lease.renew() // not held yet: nothing
        lease.begin(guard); lease.begin(guard)
        lease.renew()
        guard.releaseAll(); lease.end(); guard.releaseAll()
        assertEquals(listOf("acquire 2100000", "acquire 2100000", "release"), log)
        assertFalse(lease.held)
        lease.renew()
        assertEquals(3, log.size)
    }

    @Test fun anEarlyEndIsNotRepeatedByTheGuard() {
        val guard = ExitGuard()
        val lease = WakeLease(wake, 10)
        lease.begin(guard); lease.end(); guard.releaseAll()
        assertEquals(listOf("acquire 10", "release"), log)
    }
}
