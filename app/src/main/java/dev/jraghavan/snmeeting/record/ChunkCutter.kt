package dev.jraghavan.snmeeting.record

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale
import kotlin.math.sqrt

/** PCM samples in one AAC-LC frame (F15-FR1). Every cut and rollover lands on a frame boundary. */
const val SAMPLES_PER_FRAME = 1024

/** One encoded AAC frame; [index] counts frames from the meeting's first sample (F15-FR3). */
class AacFrame(val index: Long, val data: ByteArray)

/** Recording-clock ms of [frames] frames at [rateHz]. */
fun framesToMs(frames: Long, rateHz: Int): Long = frames * SAMPLES_PER_FRAME * 1000 / rateHz

/** The frame boundary nearest [ms] at [rateHz]. */
fun msToFrames(ms: Long, rateHz: Int): Long = (ms * rateHz + SAMPLES_PER_FRAME * 500L) / (SAMPLES_PER_FRAME * 1000L)

/** One MP4 file being written (the core's MP4 writer on the device, F15-FR1). */
interface AacFile {
    /** [ptsUs] from the file's own start. */
    fun write(data: ByteArray, ptsUs: Long)
    /** Finalise; true when the file holds at least one frame and was closed cleanly. */
    fun close(): Boolean
}

fun interface AacFiles { fun open(path: String): AacFile }

/** A finalised chunk file, for `nativeChunkReady` (F15-FR4/FR5). Times are recording-clock ms. */
data class ChunkDone(
    val index: Int, val path: String, val startOffsetMs: Long, val durationMs: Long,
    /** How much of its start repeats the previous chunk's end (0 or about 1 s). */
    val overlapMs: Long,
    /** Its audio above the pause threshold, by the core's count; -1 if unknown. */
    val speechMs: Long,
    val segIndex: Int,
    /** False when the file couldn't be written: the core re-cuts it from the segment at Stop. */
    val written: Boolean,
)

/** The core's decision for the chunk being written (`nativeChunkPlan`, F15-FR4). */
data class ChunkPlan(val cutAtMs: Long?, val overlapMs: Long, val speechMs: Long)

/** null: the planner couldn't answer (the cutter then falls back to a hard cut). */
fun interface ChunkPlanner { fun plan(rms: DoubleArray, limitMs: Long?): ChunkPlan? }

/** [ChunkPlanner] over `nativeChunkPlan`'s JSON. */
class CorePlanner(private val id: String, private val call: (String, String) -> String) : ChunkPlanner {
    override fun plan(rms: DoubleArray, limitMs: Long?): ChunkPlan? = try {
        val req = JSONObject().put("rms", JSONArray().apply { rms.forEach { put(Math.round(it * 10) / 10.0) } })
        if (limitMs != null) req.put("limitMs", limitMs)
        val o = JSONObject(call(id, req.toString()))
        if (o.has("error")) null
        else ChunkPlan(if (o.isNull("cutAtMs")) null else o.getLong("cutAtMs"), o.optLong("overlapMs", 0), o.optLong("speechMs", -1))
    } catch (t: Throwable) { // a broken core (UnsatisfiedLinkError) is a planner with no answer
        null
    }
}

/**
 * F15-FR4: the PCM RMS per 100 ms block, from the meeting's first sample. The capture thread
 * feeds it; the cutter reads the blocks of the chunk being written and drops older ones.
 */
class RmsMeter(rateHz: Int) {
    val blockSamples = maxOf(1, rateHz / 10)
    private var sum = 0.0
    private var inBlock = 0
    /** Block number of blocks[0]. */
    private var firstBlock = 0L
    private val blocks = ArrayList<Double>()

    fun onPcm(pcm: ShortArray, n: Int) {
        for (i in 0 until minOf(n, pcm.size)) {
            val s = pcm[i].toDouble()
            sum += s * s
            if (++inBlock == blockSamples) {
                blocks += sqrt(sum / blockSamples)
                sum = 0.0; inBlock = 0
            }
        }
    }

    /** Complete blocks from the one holding [fromSample] on. */
    fun from(fromSample: Long): DoubleArray {
        val start = (fromSample / blockSamples - firstBlock).coerceIn(0, blocks.size.toLong()).toInt()
        return blocks.subList(start, blocks.size).toDoubleArray()
    }

    /** Forget the blocks before the one holding [sample]. */
    fun drop(beforeSample: Long) {
        val n = (beforeSample / blockSamples - firstBlock).coerceIn(0, blocks.size.toLong()).toInt()
        if (n == 0) return
        blocks.subList(0, n).clear()
        firstBlock += n
    }
}

/**
 * F15-FR4: the chunk writer. Encoded frames are held in memory until the core says where the
 * chunk ends (at 35 s of audio, or sooner when the segment ends or the recording stops); the
 * frames up to the cut are then written as `c-NNNN.m4a` in [dir] and reported. After a hard cut
 * the next chunk starts with the last ~1 s again (the overlap the core removes when joining
 * texts); a chunk never spans a segment boundary. Plain Kotlin on the capture thread.
 *
 * A planner that can't answer gets a hard cut (35 s with 1 s overlap, or at the limit). Writing
 * a chunk file never stops the recording (INV10): a file that fails is still reported, as not
 * written, and re-cut from its segment at Stop (F15-FR9).
 */
class ChunkCutter(
    private val rateHz: Int,
    private val dir: File,
    private val planner: ChunkPlanner,
    private val files: AacFiles,
    private val rms: RmsMeter,
    private val out: (ChunkDone) -> Unit,
    private val log: (String, Throwable) -> Unit = { _, _ -> },
    /** The first chunk's index: a restarted recorder numbers on (PcmPipeline.Base). */
    firstIndex: Int = 0,
) {
    private val buf = ArrayList<AacFrame>()
    /** Frames at the start of [buf] that the previous chunk already holds. */
    private var repeated = 0
    private var segIndex = 0
    private var next = firstIndex
    private val maxFrames = msToFrames(MAX_MS, rateHz).toInt()

    /** Chunks written so far (the next chunk's index). */
    val count get() = next

    /** A frame of segment [seg]; frames arrive in order. */
    fun add(f: AacFrame, seg: Int) {
        if (buf.isEmpty()) segIndex = seg
        buf += f
        if (buf.size >= maxFrames) decide(limitMs = null)
    }

    /** The last frame added ends its segment: the chunk ends with it (gapless, no overlap). */
    fun endSegment() = forced()

    /** Stop: the last chunk ends at the stop point. */
    fun finish() = forced()

    private fun forced() {
        if (buf.size <= repeated) { buf.clear(); repeated = 0; return } // nothing new since the last cut
        decide(limitMs = framesToMs(buf.size.toLong(), rateHz))
    }

    private fun decide(limitMs: Long?) {
        val startSample = buf.first().index * SAMPLES_PER_FRAME
        val plan = planner.plan(rms.from(startSample), limitMs)
            ?: ChunkPlan(limitMs ?: MAX_MS, if (limitMs == null) OVERLAP_MS else 0, -1)
        val cutMs = plan.cutAtMs ?: return // not yet
        val cut = if (limitMs != null) buf.size else msToFrames(cutMs, rateHz).toInt().coerceIn(repeated + 1, buf.size)
        val overlap = if (plan.overlapMs > 0 && limitMs == null) msToFrames(plan.overlapMs, rateHz).toInt().coerceIn(0, cut - 1) else 0
        write(cut, plan.speechMs)
        val keep = ArrayList(buf.subList(cut - overlap, buf.size))
        buf.clear(); buf += keep
        repeated = overlap
        rms.drop(if (buf.isEmpty()) Long.MAX_VALUE / 2 else buf.first().index * SAMPLES_PER_FRAME)
    }

    private fun write(cut: Int, speechMs: Long) {
        val index = next++
        val path = File(dir, chunkName(index)).path
        val first = buf.first().index
        val ok = try {
            dir.mkdirs()
            val f = files.open(path)
            var closed = false
            try {
                for (i in 0 until cut) f.write(buf[i].data, (buf[i].index - first) * SAMPLES_PER_FRAME * 1_000_000L / rateHz)
                closed = true
                f.close()
            } finally {
                if (!closed) try { f.close() } catch (_: Throwable) {}
            }
        } catch (t: Throwable) {
            log("chunk $index", t)
            false
        }
        out(ChunkDone(index, path, framesToMs(first, rateHz), framesToMs(cut.toLong(), rateHz),
            framesToMs(repeated.toLong(), rateHz), speechMs, segIndex, ok))
    }

    companion object {
        const val MAX_MS = 35_000L
        const val OVERLAP_MS = 1_000L

        /** `c-0007.m4a`: app-private, under `<filesDir>/chunks/<meeting-id>/` (F15-FR4). */
        fun chunkName(index: Int) = "c-%04d.m4a".format(Locale.ROOT, index)
    }
}
