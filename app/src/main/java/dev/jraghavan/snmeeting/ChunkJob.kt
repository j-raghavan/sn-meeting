package dev.jraghavan.snmeeting

import android.os.Handler
import android.os.Looper
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.record.ChunkRecut
import org.json.JSONObject

/**
 * F15-FR9 / FR10: a near-live meeting's transcription, on the job lane. The chunk files the
 * core still needs but no longer has (deleted once done, or never written) are first re-cut
 * from their closed segments by remuxing the AAC frames (`nativeChunksToCut` → [cut] →
 * `nativeChunkReady`); then the core sends the pending chunks under the job rules and fills the
 * minutes once (D15). A cut that fails is left to the core, which reads that chunk as
 * "(audio missing)".
 */
class ChunkJob(
    private val toCut: (String) -> String,
    private val cut: (Cut) -> Boolean,
    private val ready: (String, Cut) -> String,
    private val transcribe: (String) -> String,
) {
    data class Cut(
        val index: Int, val segPath: String, val fromMs: Long, val toMs: Long, val path: String,
        val startOffsetMs: Long, val durationMs: Long,
    ) {
        companion object {
            fun from(o: JSONObject): Cut? = try {
                Cut(o.getInt("index"), o.getString("segPath"), o.getLong("fromMs"), o.getLong("toMs"), o.getString("path"),
                    o.getLong("startOffsetMs"), o.getLong("durationMs"))
            } catch (e: Exception) {
                null
            }
        }
    }

    /** The core's reply to the transcription (`{meeting}` or `{error}`). */
    fun run(id: String): String {
        CoreJson.list(toCut(id), Cut::from).getOrNull().orEmpty().forEach { c ->
            val ok = try { cut(c) } catch (t: Throwable) { false }
            if (ok) ready(id, c)
        }
        return transcribe(id)
    }

    companion object {
        /** On the device: [again] is Transcribe again (F15-FR10), else Transcribe / the Stop job. */
        fun device(again: Boolean) = ChunkJob(
            NativeBridge::nativeChunksToCut,
            { c -> ChunkRecut.cut(c.segPath, c.fromMs, c.toMs, c.path) },
            { id, c -> NativeBridge.nativeChunkReady(id, c.index, c.path, c.startOffsetMs, c.durationMs, -1) },
            if (again) NativeBridge::nativeTranscribeAgain else NativeBridge::nativeTranscribe,
        )
    }
}

/**
 * F15-FR9: the Stop job. When a near-live recording ends (or a killed one is recovered, F15-AC8)
 * the remaining chunks are sent and the minutes filled, with no tap: the near-live notice
 * (F15-FR12) covered sending. It runs on the job lane under the meeting's id, so the meeting
 * screen shows its progress and Cancel, and a second start is refused.
 */
object StopJob {
    private val main by lazy { Handler(Looper.getMainLooper()) }

    /** Any thread. */
    fun start(id: String) = main.post {
        LongJobs.lane.run(id, { ChunkJob.device(again = false).run(id) }) { SearchJobs.refresh() }
    }

    fun resume(ids: List<String>) = ids.forEach(::start)
}
