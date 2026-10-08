package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import java.io.File
import java.util.Locale

/** Why a recording ended: the sidecar note (F2-FR3, EC5) and the line the UI shows. */
enum class EndReason(val note: String, val message: String) {
    USER("", "Saved"),
    MIC_LOST("mic disconnected", "Microphone disconnected — recording saved"),
    STORAGE_FULL("storage full", "Storage full — recording saved"),
    RECORDER_ERROR("recorder error", "The recorder stopped — recording saved"),
    ;

    /** An ending the user didn't ask for stays in the notification shade once the service stops. */
    val leavesNotice get() = this != USER

    companion object {
        fun ofMessage(message: String): EndReason? = entries.firstOrNull { it.message == message }
    }
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
    private var armed: String? = null

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
        if (lowSpace()) return discard("Storage full")
        try {
            rec.start(spec, source, deviceId, b.firstSegmentPath, segBytes, safetyDurationMs(b), events)
        } catch (e: Exception) {
            return discard(CANT_START)
        }
        t0 = clock.elapsedMs(); index = 0; lastOffset = 0; armed = null
        phase = Phase.RECORDING
        listener.onRecording(b.id, t0)
        listener.onSegmentStarted(0)
    }

    /** User Stop, notification Stop, service onDestroy. */
    fun stop() = end(EndReason.USER)

    /** Pinned input removed, SCO lost, or audio routed elsewhere (INV1: never another mic). */
    fun onMicLost() = end(EndReason.MIC_LOST)

    private val events = object : RecorderEvents {
        override fun onApproachingLimit() {
            if (phase != Phase.RECORDING || armed != null) return
            if (lowSpace()) return // no next file: the limit then ends it as storage full (EC5)
            val next = pathFor(index + 2)
            if (rec.setNext(next)) armed = next // else the limit arrives and we stop + restart
        }

        override fun onNextFileStarted() {
            val next = armed ?: return
            if (phase != Phase.RECORDING) return
            armed = null
            beginNext(next, closePrevious = true)
        }

        override fun onLimitReached() {
            if (phase != Phase.RECORDING) return
            if (lowSpace()) return end(EndReason.STORAGE_FULL)
            // Fallback (R1): stop + restart; each gap < 1 s, measured from offsets and durations.
            closeCurrent(rec.stop())
            val next = pathFor(index + 2)
            armed = null
            try {
                rec.start(spec, source, deviceId, next, segBytes, safetyDurationMs(begun), this)
            } catch (e: Exception) {
                return finish(EndReason.RECORDER_ERROR)
            }
            beginNext(next, closePrevious = false)
        }

        override fun onError(what: Int) {
            end(if (lowSpace()) EndReason.STORAGE_FULL else EndReason.RECORDER_ERROR)
        }

        /** F2-FR3: setPreferredDevice is only a preference; any other input is mic loss. */
        override fun onRoutedTo(deviceId: Int?) {
            if (deviceId != null && deviceId != this@SegmentedRecorder.deviceId) onMicLost()
        }
    }

    private fun beginNext(next: String, closePrevious: Boolean) {
        val offset = maxOf(clock.elapsedMs() - t0, lastOffset + 1) // the core needs strictly later
        if (closePrevious) core.closeSegment(begun.id, index, pathFor(index + 1), measure = true, fallbackMs = offset - lastOffset)
        index += 1; lastOffset = offset
        core.addSegment(begun.id, next, offset) { r ->
            if (r is CoreResult.Err) end(EndReason.RECORDER_ERROR) // the library would not know this file
        }
        listener.onSegmentStarted(index)
    }

    private fun end(reason: EndReason) {
        when (phase) {
            Phase.BEGINNING -> { phase = Phase.ENDED; listener.onEnded(null, "Recording cancelled") }
            Phase.RECORDING -> {
                val finalised = rec.stop()
                closeCurrent(finalised)
                finish(reason, nothingKept = !finalised && index == 0)
            }
            else -> Unit
        }
    }

    /** F2-FR7: a stop with no audio deletes that file; the core then reads the segment as empty. */
    private fun closeCurrent(finalised: Boolean) {
        val path = pathFor(index + 1)
        if (!finalised) storage.delete(path)
        val estimate = if (finalised) clock.elapsedMs() - t0 - lastOffset else 0
        core.closeSegment(begun.id, index, path, measure = finalised, fallbackMs = estimate)
    }

    /** [nothingKept]: the only segment had no audio, so the meeting is deleted (F2-FR7). */
    private fun finish(reason: EndReason, nothingKept: Boolean = false) {
        phase = Phase.STOPPING
        core.finish(begun.id, clock.elapsedMs() - t0, reason.note, discard = nothingKept) {
            phase = Phase.ENDED
            listener.onEnded(if (nothingKept) null else begun.id, if (nothingKept) TOO_SHORT else reason.message)
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

    /** Room for another whole segment plus a margin for the container (EC5). */
    private fun lowSpace() = storage.usableBytes(begun.firstSegmentPath) < segBytes + RESERVE_BYTES

    companion object {
        const val RESERVE_BYTES = 1_000_000L
        const val CANT_START = "Couldn't start the recorder"
        const val TOO_SHORT = "Too short — nothing saved"

        /** The size at which to switch files: the plan's duration at this bitrate, capped by the plan's size. */
        fun segmentBytes(b: Begun, spec: AudioSpec): Long =
            minOf(b.maxFileBytes, b.maxDurationMs * spec.bitrateBps / 8 / 1000)

        /** MAX_DURATION stops the recorder (no switch), so it sits 10 % past the plan. */
        fun safetyDurationMs(b: Begun): Int = (b.maxDurationMs * 11 / 10).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

        fun segmentPath(firstPath: String, id: String, number: Int): String =
            File(File(firstPath).parentFile, "%s-%02d.m4a".format(Locale.ROOT, id, number)).path
    }
}
