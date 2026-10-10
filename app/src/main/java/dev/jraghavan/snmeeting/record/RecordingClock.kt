package dev.jraghavan.snmeeting.record

/**
 * F15-FR3: the recording clock. A meeting's time base is PCM samples captured ÷ sample rate,
 * from its first sample: segments, chunks, marks and ink strokes all use it (INV13). The
 * capture thread reports each buffer as it is read ([onBuffer]); any thread can then place an
 * `elapsedRealtime` moment on the same clock ([at]), so a tap or a stroke lands where the audio
 * was, and wall-clock jumps (NTP, time zones) never move it.
 *
 * The anchor (samples so far, and the elapsed time they were read at) is one immutable value
 * behind a volatile reference, so a reader never sees half an update.
 */
class RecordingClock(val sampleRateHz: Int) {
    init {
        require(sampleRateHz > 0) { "sample rate must be > 0" }
    }

    private class Anchor(val samples: Long, val elapsedMs: Long)

    @Volatile private var anchor: Anchor? = null

    /** [n] more samples were read; the last of them at [elapsedMs] (`SystemClock.elapsedRealtime`). */
    fun onBuffer(n: Int, elapsedMs: Long) {
        if (n <= 0) return
        val total = (anchor?.samples ?: 0L) + n
        anchor = Anchor(total, elapsedMs)
    }

    /** Samples captured so far. */
    val samples: Long get() = anchor?.samples ?: 0L

    /** Recording-clock ms of the audio captured so far. */
    val nowMs: Long get() = msOf(samples)

    fun msOf(samples: Long): Long = samples * 1000 / sampleRateHz

    /**
     * The recording-clock ms of the moment [elapsedMs]: the last buffer's time plus what has
     * elapsed since it was read (EC22: refreshed with every buffer, so a sleep between strokes
     * doesn't skew them). Never negative; null before the first buffer.
     */
    fun at(elapsedMs: Long): Long? {
        val a = anchor ?: return null
        return (msOf(a.samples) + (elapsedMs - a.elapsedMs)).coerceAtLeast(0)
    }

    companion object {
        /**
         * `MotionEvent.getEventTime()` is in the `uptimeMillis` base, which stops while the
         * device sleeps; `elapsedRealtime` doesn't. [uptimeNowMs] and [elapsedNowMs] read at
         * the same moment give the offset between the two (F17-FR4).
         */
        fun uptimeToElapsed(eventUptimeMs: Long, uptimeNowMs: Long, elapsedNowMs: Long): Long =
            eventUptimeMs + (elapsedNowMs - uptimeNowMs)
    }
}

/**
 * The recording under way's clock, for the UI (Mark, F15-FR11; strokes, F17-FR4). Set by the
 * near-live recorder when it starts and cleared when it stops. The Standard recorder has no PCM,
 * so there [nowMs] falls back to the time elapsed since the recording began, as its segment
 * offsets are.
 */
object LiveClock {
    @Volatile var current: RecordingClock? = null

    /** Recording-clock ms at [elapsedMs]; [startElapsedMs] is when the recording began. */
    fun nowMs(elapsedMs: Long, startElapsedMs: Long): Long =
        current?.at(elapsedMs) ?: (elapsedMs - startElapsedMs).coerceAtLeast(0)
}
