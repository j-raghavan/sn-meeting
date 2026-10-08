package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import java.io.File
import java.util.Locale

/** Why a recording ended: the sidecar note (F2-FR3, EC5) and the line the UI shows. */
enum class EndReason(val note: String, val message: String) {
    USER("", "Saved"),
    RECORDER_ERROR("recorder error", "The recorder stopped — recording saved"),
}

/**
 * F2-FR4/FR5/FR7, F2-FR3, EC5: one meeting's segmented recording, as plain Kotlin over ports.
 * Runs on the recording thread. Rollover is size-driven: MediaRecorder switches files only on
 * MAX_FILESIZE (setNextOutputFile), while MAX_DURATION stops it, so the duration limit sits
 * 10 % past the plan as a safety net that the stop-and-restart fallback handles.
 */
class SegmentedRecorder(
    private val core: RecorderCore,
    private val rec: RecorderPort,
    private val storage: StoragePort,
    private val clock: ClockPort,
    private val listener: Listener,
) {
    interface Listener {
        fun onRecording(id: String, startElapsedMs: Long)
        fun onSegmentStarted(index: Int)
        /** [id] is null when nothing was kept. [message] is user-facing. */
        fun onEnded(id: String?, message: String)
    }

    enum class Phase { IDLE, BEGINNING, RECORDING, STOPPING, ENDED }

    var phase = Phase.IDLE
        private set
    private lateinit var begun: Begun
    private lateinit var spec: AudioSpec
    private lateinit var source: InputSource
    private var deviceId = 0
    private var segBytes = 0L
    private var t0 = 0L
    private var index = 0
    private var lastOffset = 0L

    /** Segment `number` (1-based) beside the first: the core insists on exactly this path. */
    private fun pathFor(number: Int): String = segmentPath(begun.firstSegmentPath, begun.id, number)

    /** [candidates] in order of preference; the first the encoder accepts is used (F2-FR4). */
    fun start(mic: MicOption, deviceId: Int, source: InputSource, candidates: List<AudioSpec>) {
        check(phase == Phase.IDLE) { "start twice" }
        this.deviceId = deviceId; this.source = source
        phase = Phase.BEGINNING
        spec = candidates.firstOrNull { rec.accepts(it, source, deviceId) } ?: return abort(CANT_START)
        core.begin(clock.wallMs(), mic.json(), spec.json()) { r ->
            when (r) {
                is CoreResult.Err -> abort(r.message)
                is CoreResult.Ok -> onBegun(r.value)
            }
        }
    }

    private fun onBegun(b: Begun) {
        begun = b
        if (phase != Phase.BEGINNING) return cleanUp(b) // stopped while beginning: leave nothing live
        segBytes = segmentBytes(b, spec)
        try {
            rec.start(spec, source, deviceId, b.firstSegmentPath, segBytes, safetyDurationMs(b), events)
        } catch (e: Exception) {
            return discard(CANT_START)
        }
        t0 = clock.elapsedMs(); index = 0; lastOffset = 0
        phase = Phase.RECORDING
        listener.onRecording(b.id, t0)
        listener.onSegmentStarted(0)
    }

    /** User Stop, notification Stop, service onDestroy. */
    fun stop() = end(EndReason.USER)

    private val events = object : RecorderEvents {
        // Rollover (F2-FR5) comes next; until then a segment ends at its limit.
        override fun onApproachingLimit() = Unit
        override fun onNextFileStarted() = Unit
        override fun onLimitReached() = end(EndReason.RECORDER_ERROR)

        override fun onError(what: Int) {
            end(EndReason.RECORDER_ERROR)
        }

        override fun onRoutedTo(deviceId: Int?) = Unit
    }

    private fun end(reason: EndReason) {
        when (phase) {
            Phase.BEGINNING -> { phase = Phase.ENDED; listener.onEnded(null, "Recording cancelled") }
            Phase.RECORDING -> {
                rec.stop()
                closeCurrent()
                finish(reason)
            }
            else -> Unit
        }
    }

    private fun closeCurrent() {
        val estimate = clock.elapsedMs() - t0 - lastOffset
        core.closeSegment(begun.id, index, pathFor(index + 1), measure = true, fallbackMs = estimate)
    }

    private fun finish(reason: EndReason) {
        phase = Phase.STOPPING
        core.finish(begun.id, clock.elapsedMs() - t0, reason.note, discard = false) {
            phase = Phase.ENDED
            listener.onEnded(begun.id, reason.message)
        }
    }

    /** Begun in the core, but no audio was ever written: close at 0, finish, delete. */
    private fun discard(message: String) {
        phase = Phase.STOPPING
        core.closeSegment(begun.id, 0, begun.firstSegmentPath, measure = false, fallbackMs = 0)
        core.finish(begun.id, 0, "", discard = true) { phase = Phase.ENDED; listener.onEnded(null, message) }
    }

    private fun cleanUp(b: Begun) {
        core.closeSegment(b.id, 0, b.firstSegmentPath, measure = false, fallbackMs = 0)
        core.finish(b.id, 0, "", discard = true) {}
    }

    private fun abort(message: String) { phase = Phase.ENDED; listener.onEnded(null, message) }

    companion object {
        const val CANT_START = "Couldn't start the recorder"

        /** The size at which to switch files: the plan's duration at this bitrate, capped by the plan's size. */
        fun segmentBytes(b: Begun, spec: AudioSpec): Long =
            minOf(b.maxFileBytes, b.maxDurationMs * spec.bitrateBps / 8 / 1000)

        /** MAX_DURATION stops the recorder (no switch), so it sits 10 % past the plan. */
        fun safetyDurationMs(b: Begun): Int = (b.maxDurationMs * 11 / 10).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

        fun segmentPath(firstPath: String, id: String, number: Int): String =
            File(File(firstPath).parentFile, "%s-%02d.m4a".format(Locale.ROOT, id, number)).path
    }
}
