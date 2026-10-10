package dev.jraghavan.snmeeting.record

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RecordingClockTest { // F15-FR3
    @After fun clear() { LiveClock.current = null }

    @Test fun theClockIsSamplesOverTheRate() {
        val c = RecordingClock(44_100)
        assertEquals(0L, c.nowMs)
        assertNull(c.at(5_000))
        c.onBuffer(44_100, elapsedMs = 10_000)
        c.onBuffer(22_050, elapsedMs = 10_500)
        assertEquals(66_150L, c.samples)
        assertEquals(1_500L, c.nowMs)
        c.onBuffer(0, elapsedMs = 99_999) // an empty read changes nothing
        assertEquals(1_500L, c.at(10_500))
    }

    @Test fun anElapsedMomentLandsWhereTheAudioWas() {
        val c = RecordingClock(16_000)
        c.onBuffer(16_000 * 60, elapsedMs = 100_000) // one minute captured at elapsed 100 s
        assertEquals(60_000L, c.at(100_000))
        assertEquals(60_250L, c.at(100_250)) // a tap 250 ms after the last buffer
        assertEquals(59_000L, c.at(99_000)) // a stroke a second before it
        assertEquals(0L, c.at(1_000)) // never before the first sample
    }

    @Test fun eachBufferRefreshesTheMapping() { // EC22: a sleep between buffers doesn't skew it
        val c = RecordingClock(1_000)
        c.onBuffer(1_000, elapsedMs = 2_000)
        // The device slept: the next buffer is read 30 s later in elapsed time but holds 1 s.
        c.onBuffer(1_000, elapsedMs = 32_000)
        assertEquals(2_000L, c.at(32_000))
    }

    @Test fun uptimeIsConvertedToElapsed() { // F17-FR4: MotionEvent times are in the uptime base
        // Uptime 50 s and elapsed 80 s now (30 s of sleep since boot): an event at uptime 49.5 s.
        assertEquals(79_500L, RecordingClock.uptimeToElapsed(49_500, uptimeNowMs = 50_000, elapsedNowMs = 80_000))
    }

    @Test fun theUiFallsBackToElapsedWithoutPcm() {
        assertEquals(4_000L, LiveClock.nowMs(elapsedMs = 9_000, startElapsedMs = 5_000))
        assertEquals(0L, LiveClock.nowMs(elapsedMs = 4_000, startElapsedMs = 5_000))
        LiveClock.current = RecordingClock(1_000).apply { onBuffer(3_000, elapsedMs = 8_000) }
        assertEquals(3_500L, LiveClock.nowMs(elapsedMs = 8_500, startElapsedMs = 5_000))
    }

    @Test(expected = IllegalArgumentException::class) fun aZeroRateIsRefused() { RecordingClock(0) }
}
