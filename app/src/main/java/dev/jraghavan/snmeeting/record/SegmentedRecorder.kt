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
    /** Where a fault is logged: what failed and the throwable, nothing else (tag "Meetings"). */
    private val log: (String, Throwable) -> Unit = { _, _ -> },
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
    /** The file being written now: closed by this path, never a re-formatted one (N8). */
    private var currentPath = ""
    /** The current segment was closed: an end after a fault never closes (or deletes) it twice. */
    private var closed = false
    /** core.finish was called: whatever goes wrong afterwards, it is never called again. */
    private var finishing = false

    /** Segment `number` (1-based) beside the first: the core insists on exactly this path. */
    private fun pathFor(number: Int): String = segmentPath(begun.firstSegmentPath, begun.id, number)

    /** [candidates] in order of preference; the first the encoder accepts is used (F2-FR4). */
    fun start(mic: MicOption, deviceId: Int, source: InputSource, candidates: List<AudioSpec>) {
        if (phase == Phase.ENDED) return // stopped before it started (Stop while SCO connected)
        check(phase == Phase.IDLE) { "start twice" }
        this.deviceId = deviceId; this.source = source
        phase = Phase.BEGINNING
        // A probe that throws (setAudioSource can) is a spec the encoder won't take.
        spec = candidates.firstOrNull { runCatching { rec.accepts(it, source, deviceId) }.getOrDefault(false) }
            ?: return abort(CANT_START)
        core.begin(clock.wallMs(), mic.json(), spec.json()) { r ->
            safely("begin") {
                when (r) {
                    is CoreResult.Err -> abort(r.message)
                    is CoreResult.Ok -> onBegun(r.value)
                }
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
        t0 = clock.elapsedMs(); index = 0; lastOffset = 0; armed = null; currentPath = b.firstSegmentPath; closed = false
        phase = Phase.RECORDING
        listener.onRecording(b.id, t0)
        listener.onSegmentStarted(0)
    }

    /** User Stop, notification Stop, service onDestroy. */
    fun stop() = safely("stop") { end(EndReason.USER) }

    /**
     * Pinned input removed, SCO lost, or audio routed elsewhere (INV1: never another mic).
     * Routing and device removal race on an unplug: whichever comes first ends it, once.
     */
    fun onMicLost() = safely("mic lost") { end(EndReason.MIC_LOST) }

    /**
     * Something on the recorder thread threw: log it and end as a recorder error, finalising the
     * segment and finishing once. Never throws; an end that itself breaks still reports the end.
     */
    fun onFault(where: String, t: Throwable) {
        quietly { log("$where failed", t) }
        try {
            // An end that broke between stop and finish still finishes; one finishing is left be.
            if (phase == Phase.STOPPING && !finishing) finish(EndReason.RECORDER_ERROR) else end(EndReason.RECORDER_ERROR)
        } catch (t2: Throwable) {
            quietly { log("ending after a fault failed", t2) }
            if (phase != Phase.ENDED) {
                phase = Phase.ENDED
                quietly { listener.onEnded(if (::begun.isInitialized && finishing) begun.id else null, EndReason.RECORDER_ERROR.message) }
            }
        }
    }

    /** Every callback and reply on the recorder thread runs through this: a throw is a fault. */
    private inline fun safely(where: String, block: () -> Unit) {
        try { block() } catch (t: Throwable) { onFault(where, t) }
    }

    private inline fun quietly(block: () -> Unit) { try { block() } catch (_: Throwable) {} }

    private val events = object : RecorderEvents {
        override fun onApproachingLimit() {
            safely("approaching limit") {
                if (phase != Phase.RECORDING || armed != null) return
                if (lowSpace()) return // no next file: the limit then ends it as storage full (EC5)
                val next = pathFor(index + 2)
                if (rec.setNext(next)) armed = next // else the limit arrives and we stop + restart
            }
        }

        override fun onNextFileStarted() {
            safely("next file") {
                val next = armed ?: return
                if (phase != Phase.RECORDING) return
                armed = null
                beginNext(next, closePrevious = true)
            }
        }

        override fun onLimitReached() {
            safely("limit") {
                if (phase != Phase.RECORDING) return
                if (lowSpace()) return end(EndReason.STORAGE_FULL)
                // Fallback (R1): stop + restart; each gap < 1 s, measured from offsets and durations.
                closeCurrent(stopRecorder())
                val next = pathFor(index + 2)
                armed = null
                try {
                    rec.start(spec, source, deviceId, next, segBytes, safetyDurationMs(begun), this)
                } catch (e: Exception) {
                    return finish(EndReason.RECORDER_ERROR)
                }
                beginNext(next, closePrevious = false)
            }
        }

        override fun onError(what: Int) {
            safely("error $what") {
                end(if (lowSpace()) EndReason.STORAGE_FULL else EndReason.RECORDER_ERROR)
            }
        }

        /** F2-FR3: setPreferredDevice is only a preference; any other input is mic loss. */
        override fun onRoutedTo(deviceId: Int?) {
            safely("routing") {
                if (deviceId != null && deviceId != this@SegmentedRecorder.deviceId) end(EndReason.MIC_LOST)
            }
        }

        override fun onMicLost() = this@SegmentedRecorder.onMicLost()

        override fun onFault(where: String, t: Throwable) = this@SegmentedRecorder.onFault(where, t)
    }

    private fun beginNext(next: String, closePrevious: Boolean) {
        val offset = maxOf(clock.elapsedMs() - t0, lastOffset + 1) // the core needs strictly later
        if (closePrevious) core.closeSegment(begun.id, index, currentPath, measure = true, fallbackMs = offset - lastOffset)
        index += 1; lastOffset = offset; currentPath = next; closed = false
        core.addSegment(begun.id, next, offset) { r ->
            safely("add segment") {
                // The library would not know this file, or knows it by another path (N8).
                val agreed = r is CoreResult.Ok && r.value.path == next
                if (!agreed) end(EndReason.RECORDER_ERROR)
            }
        }
        listener.onSegmentStarted(index)
    }

    private fun end(reason: EndReason) {
        when (phase) {
            Phase.IDLE -> phase = Phase.ENDED // nothing began: a later start() does nothing
            Phase.BEGINNING -> { phase = Phase.ENDED; listener.onEnded(null, "Recording cancelled") }
            Phase.RECORDING -> {
                phase = Phase.STOPPING // a second end (routing and device removal race) is a no-op
                val finalised = stopRecorder()
                val nothingKept = !closed && !finalised && index == 0
                if (!closed) closeCurrent(finalised)
                finish(reason, nothingKept)
            }
            else -> Unit
        }
    }

    /**
     * The adapter's stop() doesn't throw, but if a recorder does, the file may well hold audio
     * (the on-device unplug left a playable one): keep it, measure it, estimate if unreadable.
     */
    private fun stopRecorder(): Boolean = try {
        rec.stop()
    } catch (t: Throwable) {
        quietly { log("stop failed", t) }
        true
    }

    /** F2-FR7: a stop with no audio deletes that file; the core then reads the segment as empty. */
    private fun closeCurrent(finalised: Boolean) {
        val path = currentPath
        closed = true
        if (!finalised) storage.delete(path)
        val estimate = if (finalised) clock.elapsedMs() - t0 - lastOffset else 0
        core.closeSegment(begun.id, index, path, measure = finalised, fallbackMs = estimate)
    }

    /** [nothingKept]: the only segment had no audio, so the meeting is deleted (F2-FR7). */
    private fun finish(reason: EndReason, nothingKept: Boolean = false) {
        phase = Phase.STOPPING
        finishing = true
        core.finish(begun.id, clock.elapsedMs() - t0, reason.note, discard = nothingKept) {
            phase = Phase.ENDED
            safely("ended") { listener.onEnded(if (nothingKept) null else begun.id, if (nothingKept) TOO_SHORT else reason.message) }
        }
    }

    /** Begun in the core, but no audio was ever written: close at 0, finish, delete. */
    private fun discard(message: String) {
        phase = Phase.STOPPING
        finishing = true
        core.closeSegment(begun.id, 0, begun.firstSegmentPath, measure = false, fallbackMs = 0)
        core.finish(begun.id, 0, "", discard = true) { phase = Phase.ENDED; safely("ended") { listener.onEnded(null, message) } }
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
        /**
         * The file outgrows the nominal bitrate: on the Manta a 96 kbps segment reached the 10.8 MB
         * cap at 849.6 s, as the encoded audio runs about 2 % over and the writer counts its index
         * against the cap. 5 % more puts the switch just under the plan (about 14:53 of 15:00).
         */
        const val CONTAINER_PERCENT = 105L
        const val CANT_START = "Couldn't start the recorder"
        const val TOO_SHORT = "Too short — nothing saved"

        /** The size at which to switch files: the plan's duration at this bitrate plus the container's share, capped by the plan's size. */
        fun segmentBytes(b: Begun, spec: AudioSpec): Long =
            minOf(b.maxFileBytes, b.maxDurationMs * spec.bitrateBps / 8 / 1000 * CONTAINER_PERCENT / 100)

        /** MAX_DURATION stops the recorder (no switch), so it sits 10 % past the plan. */
        fun safetyDurationMs(b: Begun): Int = (b.maxDurationMs * 11 / 10).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

        fun segmentPath(firstPath: String, id: String, number: Int): String =
            File(File(firstPath).parentFile, "%s-%02d.m4a".format(Locale.ROOT, id, number)).path
    }
}
