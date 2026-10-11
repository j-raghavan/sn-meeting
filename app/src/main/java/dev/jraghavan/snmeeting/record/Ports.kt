package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SegmentAdded

/**
 * The ports the recording logic depends on. Everything behind them is a thin Android adapter
 * (DEVICE-UNVERIFIED); everything in front is plain Kotlin with JVM tests.
 */
fun interface Cancel { fun cancel() }

/** Runs [r] on the recording thread after [ms]; the returned handle cancels it. */
fun interface TimerPort { fun after(ms: Long, r: () -> Unit): Cancel }

interface ClockPort {
    fun elapsedMs(): Long // SystemClock.elapsedRealtime()
    fun wallMs(): Long // System.currentTimeMillis()
}

/** AAC-LC mono; what goes in the sidecar's `audio` (F2-FR4). */
data class AudioSpec(val sampleRateHz: Int, val bitrateBps: Int) {
    fun json() = """{"codec":"aac-lc","sampleRateHz":$sampleRateHz,"bitrateBps":$bitrateBps,"channels":1}"""

    companion object {
        val USB = AudioSpec(44_100, 96_000)
        val BLUETOOTH = AudioSpec(16_000, 32_000)
        val BLUETOOTH_FALLBACK = AudioSpec(16_000, 48_000)
        /** F22: the built-in mic records as USB does, falling back to Bluetooth's 16 kHz. */
        val BUILT_IN = listOf(USB, AudioSpec(16_000, 48_000))
    }
}

enum class InputSource {
    MIC, VOICE_COMMUNICATION,
    /** F22-FR3: the built-in mic's sources, best first (see [BUILT_IN_SOURCES]). */
    VOICE_RECOGNITION, UNPROCESSED,
}

/**
 * F22-FR3: the built-in mic tries these in order; a device that does better with another source
 * is a change here. UNPROCESSED only where the platform says it is supported.
 */
fun builtInSources(unprocessedSupported: Boolean): List<InputSource> =
    listOfNotNull(InputSource.UNPROCESSED.takeIf { unprocessedSupported }, InputSource.VOICE_RECOGNITION, InputSource.MIC)

/** What the recorder reports (MediaRecorder OnInfo / OnError / AudioRouting). */
interface RecorderEvents {
    fun onApproachingLimit() // MEDIA_RECORDER_INFO_MAX_FILESIZE_APPROACHING (90 %)
    fun onNextFileStarted() // MEDIA_RECORDER_INFO_NEXT_OUTPUT_FILE_STARTED
    /**
     * F15-FR2: a time-driven recorder started the next file at [offsetMs] on the recording
     * clock (samples ÷ rate), not the elapsed time it was heard. MediaRecorder never calls it.
     */
    fun onNextFileStartedAt(offsetMs: Long) = onNextFileStarted()
    /** F15-FR4: a near-live chunk file was finalised. MediaRecorder never calls it. */
    fun onChunk(c: ChunkDone) {}
    fun onLimitReached() // MAX_DURATION_REACHED, or MAX_FILESIZE_REACHED with no next file
    fun onError(what: Int)
    fun onRoutedTo(deviceId: Int?) // the input actually in use
    fun onMicLost() // the input in use could not be read, or is not the pinned one (RoutingWatch)
    /** A platform callback threw: end as a recorder error, never crash the recorder thread. */
    fun onFault(where: String, t: Throwable)
}

/** One MediaRecorder at a time. All calls on the recording thread. */
interface RecorderPort {
    /** prepare() into a scratch file and release: does the encoder accept [spec]? */
    fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int): Boolean

    /** prepare() + start(); throws if either fails (the adapter releases and deletes on failure). */
    fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents)

    /** setNextOutputFile(File(path)); false if the platform refuses it (fallback: stop + restart). */
    fun setNext(path: String): Boolean

    /** stop() + release(). true = finalised; false = stop() threw with no audio captured (F2-FR7). */
    fun stop(): Boolean

    /**
     * F15-FR2 (D14): this recorder rolls over by sample count at exactly the plan's length, so
     * it is started with that length and the plan's size cap only as a runaway guard. false
     * for MediaRecorder, whose rollover is size-driven (F2-FR5).
     */
    val timeDriven: Boolean get() = false
}

interface StoragePort {
    fun usableBytes(path: String): Long
    fun delete(path: String)
}

/**
 * The library calls the recorder makes, queued in order on CoreJobs' single thread
 * (M2/M3 ruling); results come back on the recording thread. closeSegment measures the file on
 * that worker first, so a measure-then-close pair is never reordered with the next call.
 */
interface RecorderCore {
    fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit)
    fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit)
    /**
     * Measure (retrying briefly: the file may still be finalising) and close. [fallbackMs] is used
     * if the file can't be measured: the elapsed estimate for a segment known to be complete, 0
     * for one with no audio.
     */
    fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long)
    /** finish, then (if [discard]) nativeDeleteMeeting: a meeting with no audio leaves nothing behind. */
    fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit)
    /** F15-FR5: a chunk file is finalised (nativeChunkReady), queued before the next close or finish. */
    fun chunkReady(id: String, c: ChunkDone) {}
}

/** Measures a finalised file (MediaMetadataRetriever METADATA_KEY_DURATION); null if unreadable. */
fun interface DurationPort { fun durationMs(path: String): Long? }

/** The partial wakelock, not reference-counted: acquire() again only extends the timeout. */
interface WakePort {
    fun acquire(timeoutMs: Long)
    fun release()
}

/**
 * INV1 (S14): the recorder uses the pinned input or nothing. setPreferredDevice is only a
 * preference, so a pin the platform refuses, or an input already gone, is a recorder that
 * won't start; and audio routed to any other input once started is mic loss.
 */
object Pin {
    fun require(found: Boolean, preferred: Boolean) {
        check(found) { "the pinned microphone is gone" }
        check(preferred) { "the recorder refused the pinned microphone" }
    }

    /** null: not routed yet; the routing callback reports it when it is. */
    fun routedAway(pinned: Int, routed: Int?): Boolean = routed != null && routed != pinned
}
