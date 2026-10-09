package dev.jraghavan.snmeeting.record

import java.io.File

/** What the pipeline tells the recorder (posted to the recording thread by the adapter). */
interface PipelineEvents {
    fun approaching()
    /** The next segment's file began at [offsetMs] on the recording clock (F15-FR2). */
    fun nextFile(offsetMs: Long)
    /** The segment's end passed with no next file armed, or its file outgrew the size guard. */
    fun limit()
    fun chunk(c: ChunkDone)
    /** Writing the segment file failed: the recording can't go on (a recorder error). */
    fun failed(where: String, t: Throwable)
}

/**
 * F15-FR1 (ADR D14): one meeting's capture pipeline as plain Kotlin, on the capture thread. PCM
 * read from `AudioRecord` goes to the recording clock (F15-FR3) and the RMS meter (F15-FR4);
 * the AAC frames `MediaCodec` makes of it go, each once and in order, to two writers: the
 * 15-minute segment file (rolled over by sample count, F15-FR2) and the chunk being held
 * (F15-FR4). The Android objects stay behind [AacFiles] and the adapter that feeds this.
 *
 * [base] continues a meeting after the stop-and-restart fallback: frames, segments and chunks
 * count on from where the first pipeline stopped.
 */
class PcmPipeline(
    private val rateHz: Int,
    planMs: Long,
    /** The size guard (F15-FR1): past it, a segment file is a runaway, reported as the limit. */
    private val maxFileBytes: Long,
    firstPath: String,
    chunkDir: File,
    planner: ChunkPlanner,
    private val files: AacFiles,
    private val events: PipelineEvents,
    private val base: Base = Base(),
    approachMs: Long = APPROACH_MS,
    log: (String, Throwable) -> Unit = { _, _ -> },
) {
    data class Base(val frame: Long = 0, val segment: Int = 0, val chunk: Int = 0)

    val clock = RecordingClock(rateHz)
    private val rms = RmsMeter(rateHz)
    private val roller = SegmentRoller(SegmentSchedule(rateHz, planMs), msToFrames(approachMs, rateHz))
    private val cutter = ChunkCutter(rateHz, chunkDir, planner, files, rms, events::chunk, log, firstIndex = base.chunk)

    private var path = firstPath
    private var file: AacFile? = null
    private var segFrame0 = base.frame
    private var bytes = 0L
    private var guarded = false
    /** Frames since this pipeline began. */
    private var frames = 0L
    private var broken = false
    /** Any frame written to the current segment file. */
    private var wrote = false

    /** The meeting-wide frame, segment and chunk counts reached, for a restart's [Base]. */
    val reached get() = Base(base.frame + frames, base.segment + roller.segment + 1, cutter.count)

    /** Samples read: clock and RMS (any buffer size). */
    fun onPcm(pcm: ShortArray, n: Int, elapsedMs: Long) {
        clock.onBuffer(n, elapsedMs)
        rms.onPcm(pcm, n)
    }

    /** setNext, from the recording thread. */
    fun arm(next: String) = roller.arm(next)

    /** One encoded AAC frame (1 024 samples), in order. */
    fun onFrame(data: ByteArray) {
        if (broken) return
        val f = frames++
        val global = base.frame + f
        when (val step = roller.before(f)) {
            SegmentRoller.Step.Approaching -> events.approaching()
            SegmentRoller.Step.Limit -> events.limit()
            is SegmentRoller.Step.Roll -> {
                cutter.endSegment() // a chunk never spans a segment boundary
                if (!closeFile()) return
                path = step.path; segFrame0 = global; bytes = 0; guarded = false
                events.nextFile(framesToMs(global, rateHz))
            }
            SegmentRoller.Step.Write -> Unit
        }
        try {
            val out = file ?: files.open(path).also { file = it }
            out.write(data, (global - segFrame0) * SAMPLES_PER_FRAME * 1_000_000L / rateHz)
            wrote = true
        } catch (t: Throwable) {
            broken = true
            events.failed("segment write", t)
            return
        }
        bytes += data.size
        if (!guarded && bytes > maxFileBytes) { guarded = true; events.limit() }
        cutter.add(AacFrame(global, data), base.segment + roller.segment)
    }

    /** Stop: the last chunk ends here and the segment file is finalised. true if it holds audio. */
    fun finish(): Boolean {
        roller.stopping = true
        cutter.finish()
        return closeFile() && wrote
    }

    /** Finalise the current segment file; false (and the recording ends) if that fails. */
    private fun closeFile(): Boolean {
        val f = file ?: return !broken
        file = null
        return try {
            f.close()
        } catch (t: Throwable) {
            broken = true
            events.failed("segment close", t)
            false
        }
    }

    companion object {
        /** SegmentedRecorder arms the next file this long before a boundary. */
        const val APPROACH_MS = 10_000L

        /** `<filesDir>/chunks/<meeting-id>`, from a segment path `…/<id>-NN.m4a` (F15-FR4). */
        fun chunkDir(filesDir: File, segmentPath: String): File =
            File(File(filesDir, "chunks"), File(segmentPath).name.substringBeforeLast('-'))
    }
}
