package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.NativeBridge
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

// F15-FR8 / FR9 (ADR D14, revised 2026-10-09): segments and chunk files are re-cut and rebuilt
// by the core's MP4 writer (meeting-core/src/mp4.rs, host-tested and checked with ffprobe),
// never MediaMuxer, whose stop() aborts the process on the Manta's firmware.

/** F3-FR5 on the device: the core's recovery, files measured, near-live segments repaired (F15-FR8). */
fun deviceRecoverer(filesDir: File) = Recoverer(NativeBridge::nativeRecover, RetrieverDuration,
    { id, i, ms -> NativeBridge.nativeCloseSegment(id, i, ms) }, ChunkRecut.rebuilder(filesDir))

object ChunkRecut {
    /** Whether a core reply is an error envelope. */
    private fun ok(json: String) = try { !JSONObject(json).has("error") } catch (e: Exception) { false }

    /**
     * F15-FR8: a segment its writer never finished, repaired from its frame journal when there
     * is one, else rebuilt from its finished chunks, each without the overlap it repeats.
     */
    fun rebuilder(filesDir: File) = RebuildPort { pieces, target ->
        val list = JSONArray().apply { pieces.forEach { put(JSONObject().put("path", it.path).put("skipMs", it.skipMs)) } }
        ok(NativeBridge.nativeMp4Rebuild(target, Mp4Journal.forSegment(filesDir, target) ?: "", list.toString()))
    }

    /** F15-FR9 / FR10: chunk file [out] re-cut from segment [segPath]'s frames in [fromMs, toMs). */
    fun cut(segPath: String, fromMs: Long, toMs: Long, out: String): Boolean = ok(NativeBridge.nativeMp4Cut(segPath, fromMs, toMs, out))

}
