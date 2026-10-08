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
    }
}

enum class InputSource { MIC, VOICE_COMMUNICATION }

/** What the recorder reports (MediaRecorder OnInfo / OnError / AudioRouting). */
interface RecorderEvents {
    fun onApproachingLimit() // MEDIA_RECORDER_INFO_MAX_FILESIZE_APPROACHING (90 %)
    fun onNextFileStarted() // MEDIA_RECORDER_INFO_NEXT_OUTPUT_FILE_STARTED
    fun onLimitReached() // MAX_DURATION_REACHED, or MAX_FILESIZE_REACHED with no next file
    fun onError(what: Int)
    fun onRoutedTo(deviceId: Int?) // the input actually in use
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
}
