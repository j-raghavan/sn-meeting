package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.record.RecordingClock
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class StrokeTimingTest { // F17-FR4
    /**
     * F17-AC3: a recording-clock mapping and MotionEvent times in the uptime base. The device
     * has slept 40 s since boot (elapsed runs 40 s ahead of uptime); the PCM clock has 7 min 30 s
     * of audio as of elapsed 1 000 000. A pen lands 120 ms after that buffer was read.
     */
    @Test fun aStrokesFirstPointIsTheAudioTimeItWasDrawnAt() {
        val clock = RecordingClock(16_000).apply { onBuffer(16_000 * 450, elapsedMs = 1_000_000) }
        val uptimeNow = 960_200L
        val elapsedNow = 1_000_200L
        val times = StrokeTimes.recording(uptimeNow, elapsedNow, startElapsedMs = 550_000) { clock }
        val b = StrokeBuffer(times)
        b.add(10f, 20f, 0.4f, eventUptimeMs = 960_120) // elapsed 1 000 120: 120 ms after the buffer
        b.add(11f, 21f, 0.6f, eventUptimeMs = 960_136)
        b.add(12f, 23f, 1.7f, eventUptimeMs = 960_152)
        val audioAtPenDown = 450_000L + 120
        assertTrue(abs(b.recMs!! - audioAtPenDown) <= 50)
        val packed = b.pack { x, y -> (x * 2) to (y * 2 + 1000) }
        assertArrayEquals(
            floatArrayOf(20f, 1040f, 0.4f, 450_120f, 22f, 1042f, 0.6f, 450_136f, 24f, 1046f, 1f, 450_152f),
            packed, 0.001f,
        )
        assertArrayEquals(floatArrayOf(10f, 20f, 11f, 21f, 12f, 23f), b.xy(), 0f)
    }

    @Test fun theStandardRecorderTimesFromItsStart() {
        val times = StrokeTimes.recording(uptimeNowMs = 5_000, elapsedNowMs = 9_000, startElapsedMs = 4_000) { null }
        assertEquals(4_500L, times.recMs(4_500)) // elapsed 8 500, 4.5 s after Record
        assertEquals(0L, times.recMs(-10_000)) // never before the start
    }

    @Test fun anUntimedStrokeSendsMinusOne() {
        val b = StrokeBuffer(StrokeTimes.UNTIMED)
        assertTrue(b.isEmpty())
        assertNull(b.recMs)
        b.add(1f, 2f, 1f, 100)
        assertEquals(1, b.size)
        assertArrayEquals(floatArrayOf(1f, 2f, 1f, -1f), b.pack { x, y -> x to y }, 0f)
        b.clear()
        assertTrue(b.isEmpty())
    }

    @Test fun aPointWithoutATimeTakesTheStrokes() {
        var n = 0
        val b = StrokeBuffer { if (n++ == 0) 7_000L else null }
        b.add(0f, 0f, 1f, 1); b.add(1f, 1f, 1f, 2)
        assertEquals(7_000f, b.pack { x, y -> x to y }[7], 0f)
    }
}
