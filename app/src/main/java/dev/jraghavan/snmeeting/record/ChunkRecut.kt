package dev.jraghavan.snmeeting.record

import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.util.Log
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.model.Recovery
import java.io.File
import java.nio.ByteBuffer

// DEVICE-UNVERIFIED (ADR D14, R13): remuxing AAC frames between MP4 files with MediaExtractor
// and MediaMuxer, never re-encoding. Checked by the device runbook (kill a near-live recording
// mid-segment, relaunch, play the rebuilt segment). The choices it makes (which pieces, what
// to skip) come from the core and are host-tested there.

/** F3-FR5 on the device: the core's recovery, files measured, near-live segments rebuilt (F15-FR8). */
fun deviceRecoverer() = Recoverer(NativeBridge::nativeRecover, RetrieverDuration,
    { id, i, ms -> NativeBridge.nativeCloseSegment(id, i, ms) }, ChunkRecut)

/**
 * F15-FR8 / F15-FR9: chunk files and segment files remuxed into one another. [rebuild] writes a
 * segment its muxer never finalised from that segment's finished chunks, each without the
 * overlap it repeats, into a temporary file renamed over the unreadable one.
 */
object ChunkRecut : RebuildPort {
    private const val TAG = "Meetings"

    override fun rebuild(pieces: List<Recovery.Piece>, target: String): Boolean {
        val tmp = File("$target.rebuild")
        var muxer: MediaMuxer? = null
        var track = -1
        var outUs = 0L
        var written = 0
        try {
            for (p in pieces) {
                val ex = MediaExtractor()
                try {
                    ex.setDataSource(p.path)
                    val t = (0 until ex.trackCount).firstOrNull { ex.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true }
                        ?: continue
                    ex.selectTrack(t)
                    val fmt = ex.getTrackFormat(t)
                    val m = muxer ?: MediaMuxer(tmp.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4).also {
                        track = it.addTrack(fmt); it.start(); muxer = it
                    }
                    val frameUs = SAMPLES_PER_FRAME * 1_000_000L / fmt.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                    val skipUs = p.skipMs * 1000
                    val buf = ByteBuffer.allocate(64 * 1024)
                    val info = MediaCodec.BufferInfo()
                    var last = -frameUs
                    while (true) {
                        val size = ex.readSampleData(buf, 0)
                        if (size < 0) break
                        val pts = ex.sampleTime - skipUs
                        if (pts >= 0) {
                            info.set(0, size, outUs + pts, MediaCodec.BUFFER_FLAG_KEY_FRAME)
                            m.writeSampleData(track, buf, info)
                            written++
                            last = pts
                        }
                        ex.advance()
                    }
                    outUs += last + frameUs
                } finally {
                    ex.release()
                }
            }
            val m = muxer ?: return false
            muxer = null
            if (written == 0) { m.release(); return false }
            m.stop(); m.release()
            return tmp.renameTo(File(target))
        } catch (t: Throwable) {
            Log.w(TAG, "rebuild: ${t.javaClass.simpleName}")
            return false
        } finally {
            try { muxer?.release() } catch (_: Throwable) {}
            if (tmp.exists()) tmp.delete()
        }
    }
}
