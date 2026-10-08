package dev.jraghavan.snmeeting

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.MediaRecorder
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Records one meeting at a time to [outputDir] as AAC in an .m4a container.
 * Records only from a USB microphone: the Supernote has no usable built-in mic
 * (its "Built-In Mic" input yields noise).
 */
class MeetingRecorder(private val context: Context, private val outputDir: File) {

    private var recorder: MediaRecorder? = null
    var currentFile: File? = null
        private set

    val isRecording: Boolean get() = recorder != null

    /** The USB input device if one is attached, else null. */
    fun usbInput(): AudioDeviceInfo? {
        val am = context.getSystemService(AudioManager::class.java)
        return am.getDevices(AudioManager.GET_DEVICES_INPUTS).firstOrNull {
            it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET
        }
    }

    fun start(): File {
        check(recorder == null) { "already recording" }
        val mic = usbInput() ?: throw IllegalStateException("no USB microphone connected")
        if (!outputDir.isDirectory && !outputDir.mkdirs()) {
            throw IllegalStateException("cannot create $outputDir")
        }
        val file = File(outputDir, fileNameFor(Date()))
        // MediaRecorder(Context) is API 31+; the Supernote runs Android 11 (API 30).
        @Suppress("DEPRECATION")
        val r = MediaRecorder()
        try {
            r.setAudioSource(MediaRecorder.AudioSource.MIC)
            r.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            r.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            r.setAudioChannels(1)
            r.setAudioSamplingRate(44_100)
            r.setAudioEncodingBitRate(96_000)
            r.setOutputFile(file.absolutePath)
            r.setPreferredDevice(mic)
            r.prepare()
            r.start()
        } catch (e: Exception) {
            r.release()
            file.delete()
            throw e
        }
        recorder = r
        currentFile = file
        return file
    }

    /** Stops and finalises the file. Returns it, or null if nothing was being recorded. */
    fun stop(): File? {
        val r = recorder ?: return null
        val file = currentFile
        recorder = null
        currentFile = null
        try {
            r.stop()
        } catch (e: RuntimeException) {
            // stop() throws when no audio was captured (stopped immediately) — the file is unusable.
            file?.delete()
            r.release()
            return null
        }
        r.release()
        return file
    }

    companion object {
        fun fileNameFor(date: Date): String =
            "meeting-" + SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(date) + ".m4a"
    }
}
