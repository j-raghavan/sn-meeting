package dev.jraghavan.snmeeting.record

/**
 * F15-FR2: time-driven, gapless rollover for the near-live recorder (D14). Segment k ends after
 * exactly (k+1) × the plan's length of PCM, counted in samples, at the first AAC frame boundary
 * at or after it; the next segment starts with the next frame, so every boundary gap is 0 by
 * construction (F2-AC3, F15-AC7: within one frame). Boundaries are computed from the meeting's
 * first sample, so rounding to frames never drifts.
 */
class SegmentSchedule(private val rateHz: Int, planMs: Long) {
    init {
        require(rateHz > 0 && planMs > 0) { "rate and plan must be > 0" }
    }

    private val planSamples = planMs * rateHz / 1000

    /** The first frame of segment [k] (0-based). */
    fun startFrame(k: Int): Long = if (k <= 0) 0 else ceilDiv(k * planSamples, SAMPLES_PER_FRAME.toLong())

    /** Segment [k]'s `startOffsetMs` on the recording clock (F15-FR3). */
    fun startOffsetMs(k: Int): Long = framesToMs(startFrame(k), rateHz)

    /** The segment frame [f] belongs to. */
    fun segmentOf(f: Long): Int {
        var k = (f * SAMPLES_PER_FRAME / planSamples).toInt()
        while (k > 0 && startFrame(k) > f) k--
        while (startFrame(k + 1) <= f) k++
        return k
    }

    private fun ceilDiv(a: Long, b: Long) = (a + b - 1) / b
}

/**
 * F15-FR2's rollover as the capture thread sees it, frame by frame. SegmentedRecorder arms the
 * next file (`setNext`) when told the limit is approaching; at the boundary the roller says
 * roll. Unarmed at the boundary (no room for another segment, EC5), it reports the limit once
 * and keeps writing the current file, as MediaRecorder would stop: SegmentedRecorder ends the
 * recording then, or arms late and the roll happens at the next frame.
 */
class SegmentRoller(private val schedule: SegmentSchedule, private val approachFrames: Long) {
    sealed class Step {
        object Write : Step()
        object Approaching : Step()
        object Limit : Step()
        /** Frame [frame] starts segment [segment] in [path]. */
        data class Roll(val segment: Int, val path: String) : Step()
    }

    var segment = 0
        private set
    @Volatile private var armed: String? = null
    private var approached = false
    private var limited = false
    /** Stopping: the last frames go to the current file, never a new one SegmentedRecorder can't hear of. */
    @Volatile var stopping = false

    /** setNext, from the recorder thread. */
    fun arm(path: String) { armed = path }

    /** What to do before writing frame [f] (frames arrive in order). Every step writes [f]; a
     *  [Step.Roll] writes it as the first frame of the new file. */
    fun before(f: Long): Step {
        val end = schedule.startFrame(segment + 1)
        if (f >= end && !stopping) {
            val next = armed
            if (next != null) {
                armed = null; approached = false; limited = false
                segment++
                return Step.Roll(segment, next)
            }
            if (!limited) { limited = true; return Step.Limit }
            return Step.Write
        }
        if (!approached && f >= end - approachFrames) { approached = true; return Step.Approaching }
        return Step.Write
    }
}
