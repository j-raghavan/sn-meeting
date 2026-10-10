package dev.jraghavan.snmeeting.record

import org.json.JSONObject
import java.io.File
import java.io.IOException

/**
 * F15-FR1 (ADR D14, revised after the device spike 2026-10-09): the near-live recorder's files
 * are written by the core's MP4 writer, not MediaMuxer, whose stop() aborts any app process on
 * the Manta's firmware (its writer thread asks for SCHED_FIFO 99 and is refused). The AAC
 * encoder's frames go to the core one by one; the core writes the index at close, and keeps a
 * journal of frame sizes for a segment so a crash leaves it repairable (F15-FR8).
 */
interface Mp4Port {
    fun open(path: String, rateHz: Int, channels: Int, asc: ByteArray, journal: String): String
    fun write(handle: Long, frame: ByteArray): String
    fun close(handle: Long): String
    fun abandon(handle: Long): String
}

/** The encoder's output format, as the writer needs it: rate, channels and csd-0 (the ASC). */
class AacConfig(val rateHz: Int, val channels: Int, val asc: ByteArray)

/** Where a segment's frame journal lives: beside its chunks, app-private; chunk files have none. */
object Mp4Journal {
    /** `<filesDir>/chunks/<meeting-id>/seg-NN.idx` for segment `…/<meeting-id>-NN.m4a`; null for a chunk file. */
    fun forSegment(filesDir: File, path: String): String? {
        val name = File(path).name
        if (name.startsWith("c-") || !name.endsWith(".m4a")) return null
        val nn = name.removeSuffix(".m4a").substringAfterLast('-')
        return File(PcmPipeline.chunkDir(filesDir, path), "seg-$nn.idx").path
    }
}

/** [AacFiles] over the core's writer. A file the core can't open or write is an IOException. */
class CoreAacFiles(
    private val config: () -> AacConfig?,
    private val port: Mp4Port,
    private val journalFor: (String) -> String?,
) : AacFiles {
    override fun open(path: String): AacFile {
        val c = config() ?: throw IOException("no encoder format yet")
        val reply = JSONObject(port.open(path, c.rateHz, c.channels, c.asc, journalFor(path) ?: ""))
        if (reply.has("error")) throw IOException(reply.optString("error"))
        val handle = reply.getLong("handle")
        return object : AacFile {
            private var n = 0
            private var done = false

            override fun write(data: ByteArray, ptsUs: Long) {
                val r = port.write(handle, data)
                if (r.isNotEmpty()) throw IOException(JSONObject(r).optString("error", r))
                n++
            }

            /** The index is written; a file with no frame is removed by the core (false). */
            override fun close(): Boolean {
                if (done) return n > 0
                done = true
                if (n == 0) { port.abandon(handle); File(path).delete(); return false }
                val r = JSONObject(port.close(handle))
                if (r.has("error")) throw IOException(r.optString("error"))
                return true
            }
        }
    }
}
