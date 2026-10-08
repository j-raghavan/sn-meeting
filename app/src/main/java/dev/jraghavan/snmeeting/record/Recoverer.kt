package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.Recovery

/**
 * F3-FR5, the shell's half of crash recovery: nativeRecover moves stuck meetings on and lists
 * the segments that were never closed (legacy audio with no sidecar included, D3); each is
 * measured from its file and closed, at 0 ms if it can't be read (the core then flags it). Runs on the core's worker thread, so the
 * measure-then-close pairs stay in order with every other library call.
 */
class Recoverer(
    private val recover: (serviceAlive: Boolean) -> String,
    private val duration: DurationPort,
    private val close: (id: String, index: Int, ms: Long) -> Unit,
) {
    /** The core's reply, after every listed segment is closed; null if it couldn't be read. */
    fun run(serviceAlive: Boolean): Recovery? {
        val r = CoreJson.obj(recover(serviceAlive), Recovery::from).getOrNull() ?: return null
        for (m in r.measure) close(m.id, m.index, duration.durationMs(m.path) ?: 0)
        return r
    }
}
