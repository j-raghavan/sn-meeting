package dev.jraghavan.snmeeting.record

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.MediaMetadataRetriever
import android.media.MediaRecorder
import android.os.Handler
import android.os.SystemClock
import dev.jraghavan.snmeeting.CoreJobs
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SegmentAdded
import java.io.File

// DEVICE-UNVERIFIED: every class in this file is a thin Android adapter over the ports in
// Ports.kt. The logic they feed is host-tested; these are checked by the on-device runbooks.

object AndroidClock : ClockPort {
    override fun elapsedMs() = SystemClock.elapsedRealtime()
    override fun wallMs() = System.currentTimeMillis()
}

class HandlerTimer(private val h: Handler) : TimerPort {
    override fun after(ms: Long, r: () -> Unit): Cancel {
        val task = Runnable(r)
        h.postDelayed(task, ms)
        return Cancel { h.removeCallbacks(task) }
    }
}

/** Every USB input (the spike's usbInput(), all of them). */
fun usbInputs(am: AudioManager): List<UsbInput> = am.getDevices(AudioManager.GET_DEVICES_INPUTS)
    .filter { it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET }
    .map { UsbInput(it.id, it.productName?.toString().orEmpty().ifBlank { "USB microphone" }) }

fun inputById(am: AudioManager, id: Int): AudioDeviceInfo? =
    am.getDevices(AudioManager.GET_DEVICES_INPUTS).firstOrNull { it.id == id }

class MediaRecorderAdapter(private val ctx: Context) : RecorderPort {
    private val am = ctx.getSystemService(AudioManager::class.java)
    private var r: MediaRecorder? = null
    private var path: String? = null

    @Suppress("DEPRECATION") // MediaRecorder(Context) is API 31+ and crashed on this device (F2 Facts)
    private fun configured(spec: AudioSpec, source: InputSource, deviceId: Int, out: String): MediaRecorder =
        MediaRecorder().apply {
            setAudioSource(if (source == InputSource.MIC) MediaRecorder.AudioSource.MIC else MediaRecorder.AudioSource.VOICE_COMMUNICATION)
            setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            setAudioChannels(1)
            setAudioSamplingRate(spec.sampleRateHz)
            setAudioEncodingBitRate(spec.bitrateBps)
            setOutputFile(out)
            inputById(am, deviceId)?.let { setPreferredDevice(it) }
        }

    override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int): Boolean {
        val probe = File(ctx.cacheDir, "probe.m4a")
        val m = configured(spec, source, deviceId, probe.path)
        return try { m.prepare(); true } catch (e: Exception) { false } finally { m.release(); probe.delete() }
    }

    override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) {
        check(r == null) { "already recording" }
        val m = configured(spec, source, deviceId, path)
        try {
            m.setMaxFileSize(maxFileBytes)
            m.setMaxDuration(maxDurationMs)
            m.setOnInfoListener { _, what, _ ->
                when (what) {
                    MediaRecorder.MEDIA_RECORDER_INFO_MAX_FILESIZE_APPROACHING -> events.onApproachingLimit()
                    MediaRecorder.MEDIA_RECORDER_INFO_NEXT_OUTPUT_FILE_STARTED -> events.onNextFileStarted()
                    MediaRecorder.MEDIA_RECORDER_INFO_MAX_DURATION_REACHED,
                    MediaRecorder.MEDIA_RECORDER_INFO_MAX_FILESIZE_REACHED -> events.onLimitReached()
                }
            }
            m.setOnErrorListener { _, what, _ -> events.onError(what) }
            m.prepare()
            m.start()
        } catch (e: Exception) {
            m.release(); File(path).delete(); throw e
        }
        r = m; this.path = path
    }

    override fun setNext(path: String): Boolean = try {
        r?.setNextOutputFile(File(path)); r != null
    } catch (e: Exception) {
        false
    }

    override fun stop(): Boolean {
        val m = r ?: return false
        r = null
        val ok = try {
            m.stop(); true
        } catch (e: RuntimeException) {
            // stop() also throws after an automatic stop at a limit: keep a file with audio in it.
            (path?.let { File(it).length() } ?: 0L) > NO_AUDIO_BYTES
        }
        m.release()
        return ok
    }

    companion object { const val NO_AUDIO_BYTES = 4096L }
}

object FileStorage : StoragePort {
    override fun usableBytes(path: String) = File(path).parentFile?.usableSpace ?: 0L
    override fun delete(path: String) { File(path).delete() }
}

object RetrieverDuration : DurationPort {
    override fun durationMs(path: String): Long? {
        val m = MediaMetadataRetriever()
        return try {
            m.setDataSource(path)
            m.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()
        } catch (e: Exception) {
            null
        } finally {
            m.release()
        }
    }
}

/** RecorderCore over NativeBridge on CoreJobs' thread; results back on the recording handler. */
class CoreRecorderBridge(private val h: Handler, private val duration: DurationPort) : RecorderCore {
    private fun <T> call(work: () -> T, done: (T) -> Unit) = CoreJobs.exec { val v = work(); h.post { done(v) } }

    override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) =
        call({ CoreJson.obj(NativeBridge.nativeBeginMeeting(startedAtMs, micJson, audioJson), Begun::from) }, done)

    override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) =
        call({ CoreJson.obj(NativeBridge.nativeAddSegment(id, path, startOffsetMs), SegmentAdded::from) }, done)

    override fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long) = CoreJobs.exec {
        NativeBridge.nativeCloseSegment(id, index, (if (measure) measured(duration, path) else null) ?: fallbackMs)
    }

    override fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit) = call({
        NativeBridge.nativeFinishMeeting(id, durationMs, note)
        if (discard) NativeBridge.nativeDeleteMeeting(id)
    }, { done() })

    companion object {
        /** The file may still be finalising: up to 5 tries, 300 ms apart, on the worker thread. */
        fun measured(duration: DurationPort, path: String): Long? {
            repeat(5) { attempt ->
                duration.durationMs(path)?.let { return it }
                if (attempt < 4) Thread.sleep(300)
            }
            return null
        }
    }
}
