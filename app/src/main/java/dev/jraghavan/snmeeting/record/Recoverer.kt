package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.Recovery

/** F15-FR8: rebuild segment file [target] from [pieces] by remuxing their AAC frames; true if it was written. */
fun interface RebuildPort { fun rebuild(pieces: List<Recovery.Piece>, target: String): Boolean }

/**
 * F3-FR5, the shell's half of crash recovery: nativeRecover moves stuck meetings on and lists
 * the segments that were never closed (legacy audio, with or without a sidecar, in any status
 * included, D3); each is measured from its file and closed, at 0 ms if it can't be read (the
 * core then flags it). Runs on the core's worker thread, so the measure-then-close pairs stay in
 * order with every other library call.
 */
class Recoverer(
    private val recover: (serviceAlive: Boolean) -> String,
    private val duration: DurationPort,
    private val close: (id: String, index: Int, ms: Long) -> Unit,
    /** F15-FR8: a near-live segment its muxer never finalised is rebuilt from its chunk files. */
    private val rebuild: RebuildPort = RebuildPort { _, _ -> false },
) {
    /** The core's reply, after every listed segment is closed; null if it couldn't be read. */
    fun run(serviceAlive: Boolean): Recovery? {
        val r = CoreJson.obj(recover(serviceAlive), Recovery::from).getOrNull() ?: return null
        for (m in r.measure) close(m.id, m.index, measure(m))
        return r
    }

    /**
     * The file's length; if it can't be read and the segment has finished chunks (a near-live
     * recording killed mid-segment), the file is rebuilt from them first, so at most the chunk
     * being written is lost (F15-FR8, INV3). 0 when nothing can be read.
     */
    private fun measure(m: Recovery.ToMeasure): Long {
        duration.durationMs(m.path)?.let { return it }
        if (m.chunks.isEmpty()) return 0
        val rebuilt = try { rebuild.rebuild(m.chunks, m.path) } catch (t: Throwable) { false }
        return if (rebuilt) duration.durationMs(m.path) ?: 0 else 0
    }
}
