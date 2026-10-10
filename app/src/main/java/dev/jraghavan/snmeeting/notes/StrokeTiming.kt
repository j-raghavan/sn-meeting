package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.record.RecordingClock

/**
 * F17-FR4: how a `MotionEvent` time (the `uptimeMillis` base) becomes recording-clock ms.
 * `null` is untimed: a stroke written when nothing is recording, or after Stop.
 */
fun interface StrokeTimes {
    fun recMs(eventUptimeMs: Long): Long?

    companion object {
        /** Nothing is recording: every stroke is untimed. */
        val UNTIMED = StrokeTimes { null }

        /**
         * The recording under way. [uptimeNowMs] and [elapsedNowMs] are read together when the
         * pen lands, which fixes the uptime-to-elapsed offset for the stroke (the device can't
         * sleep mid-stroke); [clock] is the PCM clock (null with the Standard recorder, whose
         * times are elapsed since [startElapsedMs], as Mark's are).
         */
        fun recording(uptimeNowMs: Long, elapsedNowMs: Long, startElapsedMs: Long, clock: () -> RecordingClock?): StrokeTimes =
            StrokeTimes { up ->
                val elapsed = RecordingClock.uptimeToElapsed(up, uptimeNowMs, elapsedNowMs)
                clock()?.at(elapsed) ?: (elapsed - startElapsedMs).coerceAtLeast(0)
            }
    }
}

/**
 * One pen stroke as captured: view-space points with pressure and event time, batched (the
 * historical samples included, unlike inkread's xy-only buffer, `StylusInkController.kt:190-222`).
 * [pack] gives what `nativeInkStroke` takes: `[x, y, pressure, t]` per point in world
 * coordinates, `t` on the recording clock or -1 (untimed).
 *
 * `t` travels as a Float: whole milliseconds are exact up to 2^24 ms (4 h 39 min), and a
 * longer meeting's stroke times round to 2 ms, far inside F17-AC3's 50 ms.
 */
class StrokeBuffer(private val times: StrokeTimes) {
    private val xs = ArrayList<Float>()
    private val ys = ArrayList<Float>()
    private val ps = ArrayList<Float>()
    private val ts = ArrayList<Long?>()

    val size: Int get() = xs.size
    fun isEmpty() = xs.isEmpty()

    fun add(x: Float, y: Float, pressure: Float, eventUptimeMs: Long) {
        xs += x; ys += y; ps += pressure.coerceIn(0f, 1f)
        ts += times.recMs(eventUptimeMs)
    }

    /** The view-space points, interleaved `[x, y, …]` (the live band, the lasso loop). */
    fun xy(): FloatArray = FloatArray(size * 2) { i -> if (i % 2 == 0) xs[i / 2] else ys[i / 2] }

    /** The first point's recording-clock time: the stroke's (F17-FR4); null untimed. */
    val recMs: Long? get() = ts.firstOrNull()

    fun pack(toWorld: (Float, Float) -> Pair<Float, Float>): FloatArray {
        val out = FloatArray(size * 4)
        val timed = recMs != null
        for (i in 0 until size) {
            val (wx, wy) = toWorld(xs[i], ys[i])
            out[i * 4] = wx; out[i * 4 + 1] = wy; out[i * 4 + 2] = ps[i]
            out[i * 4 + 3] = if (timed) (ts[i] ?: recMs!!).toFloat() else -1f
        }
        return out
    }

    fun clear() { xs.clear(); ys.clear(); ps.clear(); ts.clear() }
}

/** F21-FR7 [Recommendation]: a committed stroke's width at a point: `width × (0.5 + 0.5 × pressure)`. */
object PressureWidth {
    fun of(width: Float, pressure: Float): Float = width * (0.5f + 0.5f * pressure.coerceIn(0f, 1f))
}
