package dev.jraghavan.snmeeting.record

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioRouting
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.Process
import android.os.SystemClock
import android.util.Log
import dev.jraghavan.snmeeting.NativeBridge
import java.io.File
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentLinkedQueue

// DEVICE-UNVERIFIED (ADR D14 is Proposed, pending the owner's device spike, R13): every class
// in this file is a thin Android adapter for the near-live recorder. The logic they feed
// (PcmPipeline, SegmentRoller, ChunkCutter, RecordingClock, chunk::plan) is host-tested; these
// are checked by the device runbook: the AAC encoder accepting 44.1 kHz / 96 kbps and
// 16 kHz / 32 kbps, 60 minutes gapless, the battery against NFR6, and chunk files after a kill.

private const val MIME = MediaFormat.MIMETYPE_AUDIO_AAC
private const val TAG = "Meetings"

/** The AAC-LC mono encoder's format for [spec] (F2-FR4). */
private fun encoderFormat(spec: AudioSpec): MediaFormat =
    MediaFormat.createAudioFormat(MIME, spec.sampleRateHz, 1).apply {
        setInteger(MediaFormat.KEY_AAC_PROFILE, MediaCodecInfo.CodecProfileLevel.AACObjectLC)
        setInteger(MediaFormat.KEY_BIT_RATE, spec.bitrateBps)
        setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 16 * 1024)
    }

/** The core's MP4 writer over JNI (F15-FR1; never MediaMuxer, which aborts on the Manta). */
object NativeMp4 : Mp4Port {
    override fun open(path: String, rateHz: Int, channels: Int, asc: ByteArray, journal: String) =
        NativeBridge.nativeMp4Open(path, rateHz, channels, asc, journal)
    override fun write(handle: Long, frame: ByteArray) = NativeBridge.nativeMp4Write(handle, frame)
    override fun close(handle: Long) = NativeBridge.nativeMp4Close(handle)
    override fun abandon(handle: Long) = NativeBridge.nativeMp4Abandon(handle)
}

/** The encoder's output format as the writer needs it: csd-0 is the AudioSpecificConfig. */
private fun aacConfig(f: MediaFormat?): AacConfig? {
    f ?: return null
    val csd = f.getByteBuffer("csd-0") ?: return null
    val asc = ByteArray(csd.remaining()).also { csd.duplicate().get(it) }
    return AacConfig(f.getInteger(MediaFormat.KEY_SAMPLE_RATE), f.getInteger(MediaFormat.KEY_CHANNEL_COUNT), asc)
}

/**
 * F15-FR1 (D14) behind RecorderPort: `AudioRecord` (16-bit mono PCM, pinned with
 * setPreferredDevice and watched for routing, INV1) → one `MediaCodec` AAC-LC encoder → the
 * segment file and the chunk files, via [PcmPipeline] on a capture thread of its own. Time-driven
 * (F15-FR2). Pipeline events are queued and delivered on the recording [handler]; [stop] also
 * delivers what is queued, so the last chunk is reported before the meeting is finished.
 */
class PcmRecorderAdapter(private val ctx: Context, private val handler: Handler) : RecorderPort {
    override val timeDriven = true

    private val am = ctx.getSystemService(AudioManager::class.java)
    private var record: AudioRecord? = null
    private var codec: MediaCodec? = null
    private var thread: Thread? = null
    private var pipeline: PcmPipeline? = null
    private var watch: RoutingWatch? = null
    private var routing: AudioRouting.OnRoutingChangedListener? = null
    @Volatile private var running = false
    @Volatile private var format: MediaFormat? = null
    @Volatile private var finished = false
    private val pending = ConcurrentLinkedQueue<() -> Unit>()
    /** Where a stop-and-restart (SegmentedRecorder's fallback) numbers on from. */
    private var base = PcmPipeline.Base()
    private var events: RecorderEvents? = null

    private fun minBuffer(spec: AudioSpec) =
        AudioRecord.getMinBufferSize(spec.sampleRateHz, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)

    /** The PCM format is readable and the encoder takes [spec] (F2-FR4: else the 48 kbps fallback). */
    override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int): Boolean = try {
        if (minBuffer(spec) <= 0 || inputById(am, deviceId) == null) false
        else {
            val c = MediaCodec.createEncoderByType(MIME)
            try { c.configure(encoderFormat(spec), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE); true } finally { c.release() }
        }
    } catch (e: Exception) {
        false
    }

    override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) {
        check(thread == null) { "already recording" }
        this.events = events
        finished = false; format = null
        val bufBytes = maxOf(minBuffer(spec) * 4, spec.sampleRateHz) // ≥ 0.5 s of 16-bit PCM
        // Home asks for the mic before ＋ New starts the service; a start without it is refused.
        if (ctx.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            throw SecurityException("RECORD_AUDIO is not granted")
        }
        val ar = AudioRecord.Builder()
            .setAudioSource(source.platform())
            .setAudioFormat(AudioFormat.Builder().setSampleRate(spec.sampleRateHz)
                .setChannelMask(AudioFormat.CHANNEL_IN_MONO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
            .setBufferSizeInBytes(bufBytes)
            .build()
        var c: MediaCodec? = null
        try {
            check(ar.state == AudioRecord.STATE_INITIALIZED) { "AudioRecord not initialised" }
            val input = inputById(am, deviceId)
            Pin.require(found = input != null, preferred = input != null && ar.setPreferredDevice(input)) // INV1
            c = MediaCodec.createEncoderByType(MIME).also {
                it.configure(encoderFormat(spec), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                it.start()
            }
            ar.startRecording()
            check(ar.recordingState == AudioRecord.RECORDSTATE_RECORDING) { "AudioRecord didn't start" }
        } catch (e: Exception) {
            try { c?.release() } catch (_: Throwable) {}
            ar.release(); File(path).delete(); throw e
        }
        val p = PcmPipeline(spec.sampleRateHz, maxDurationMs.toLong(), maxFileBytes, path,
            PcmPipeline.chunkDir(ctx.filesDir, path), CorePlanner(meetingId(path), NativeBridge::nativeChunkPlan),
            CoreAacFiles({ aacConfig(format) }, NativeMp4) { p -> Mp4Journal.forSegment(ctx.filesDir, p) },
            pipelineEvents(events), base,
            log = { what, t -> Log.w(TAG, "$what: ${t.javaClass.simpleName}") })
        record = ar; codec = c; pipeline = p
        LiveClock.current = p.clock
        // F2-FR3: the input actually in use; reading it throwing, or another input, is mic loss.
        val w = RoutingWatch(deviceId) { ar.routedDevice?.id }
        watch = w
        val l = AudioRouting.OnRoutingChangedListener { _ -> routed(w, events, "routing") }
        try { ar.addOnRoutingChangedListener(l, handler); routing = l } catch (t: Throwable) { Log.w(TAG, "routing listener: ${t.javaClass.simpleName}") }
        handler.post { routed(w, events, "routing at start") }
        running = true
        thread = Thread({ capture(ar, c!!, p, spec.sampleRateHz) }, "pcm-capture").also { it.start() }
    }

    private fun routed(w: RoutingWatch, events: RecorderEvents, where: String) = guarded(events, where) {
        if (w.check() == RoutingWatch.Verdict.MIC_LOST) events.onMicLost()
    }

    /** Pipeline events, delivered in order on the recording thread. */
    private fun pipelineEvents(e: RecorderEvents) = object : PipelineEvents {
        override fun approaching() = post { e.onApproachingLimit() }
        override fun nextFile(offsetMs: Long) = post { e.onNextFileStartedAt(offsetMs) }
        override fun limit() = post { e.onLimitReached() }
        override fun chunk(c: ChunkDone) = post { e.onChunk(c) }
        override fun failed(where: String, t: Throwable) = post { e.onFault(where, t) }
    }

    private fun post(r: () -> Unit) {
        pending.add(r)
        handler.post { deliver() }
    }

    private fun deliver() {
        val e = events ?: return
        while (true) {
            val r = pending.poll() ?: return
            guarded(e, "pcm event") { r() }
        }
    }

    /** The capture thread: read PCM, feed the encoder, hand its frames to the pipeline. */
    private fun capture(ar: AudioRecord, c: MediaCodec, p: PcmPipeline, rate: Int) {
        try { Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO) } catch (_: Throwable) {}
        val buf = ShortArray(rate / 10) // 100 ms
        val info = MediaCodec.BufferInfo()
        var fed = 0L
        try {
            while (running) {
                val n = ar.read(buf, 0, buf.size)
                if (n < 0) {
                    if (running) post { if (n == AudioRecord.ERROR_DEAD_OBJECT) events?.onMicLost() else events?.onError(n) }
                    break
                }
                if (n == 0) continue
                p.onPcm(buf, n, SystemClock.elapsedRealtime())
                fed = feed(c, info, p, buf, n, fed, rate)
                drain(c, info, p, eos = false)
            }
            endOfStream(c, info, p, fed, rate)
            drain(c, info, p, eos = true)
        } catch (t: Throwable) {
            if (running) post { events?.onFault("capture", t) }
        } finally {
            finished = try { p.finish() } catch (t: Throwable) { false }
        }
    }

    private fun feed(c: MediaCodec, info: MediaCodec.BufferInfo, p: PcmPipeline, buf: ShortArray, n: Int, fedIn: Long, rate: Int): Long {
        var fed = fedIn
        var off = 0
        var waits = 0
        while (off < n) {
            val i = c.dequeueInputBuffer(10_000)
            if (i < 0) {
                check(++waits < 200) { "encoder input stalled" }
                drain(c, info, p, eos = false)
                continue
            }
            val ib = checkNotNull(c.getInputBuffer(i))
            ib.clear()
            val count = minOf(n - off, ib.remaining() / 2)
            ib.order(ByteOrder.LITTLE_ENDIAN).asShortBuffer().put(buf, off, count)
            c.queueInputBuffer(i, 0, count * 2, fed * 1_000_000L / rate, 0)
            fed += count; off += count
        }
        return fed
    }

    private fun endOfStream(c: MediaCodec, info: MediaCodec.BufferInfo, p: PcmPipeline, fed: Long, rate: Int) {
        repeat(100) {
            val i = c.dequeueInputBuffer(10_000)
            if (i >= 0) { c.queueInputBuffer(i, 0, 0, fed * 1_000_000L / rate, MediaCodec.BUFFER_FLAG_END_OF_STREAM); return }
            drain(c, info, p, eos = false)
        }
    }

    /** Every output frame so far ([eos]: until end of stream, at most ~1 s of waiting). */
    private fun drain(c: MediaCodec, info: MediaCodec.BufferInfo, p: PcmPipeline, eos: Boolean) {
        var idle = 0
        while (true) {
            val o = c.dequeueOutputBuffer(info, if (eos) 10_000 else 0)
            when {
                o == MediaCodec.INFO_TRY_AGAIN_LATER -> if (!eos || ++idle > 100) return
                o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> format = c.outputFormat
                o >= 0 -> {
                    val config = info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0
                    if (!config && info.size > 0) {
                        val ob = checkNotNull(c.getOutputBuffer(o))
                        val bytes = ByteArray(info.size)
                        ob.position(info.offset); ob.get(bytes, 0, info.size)
                        p.onFrame(bytes) // the encoder hands out one AAC frame (1 024 samples) per buffer
                    }
                    c.releaseOutputBuffer(o, false)
                    if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) return
                }
            }
        }
    }

    override fun setNext(path: String): Boolean {
        val p = pipeline ?: return false
        p.arm(path)
        return true
    }

    /** Never throws. Stops capture, finalises the last chunk and the segment, and delivers the
     *  queued events (the last chunk) before returning. */
    override fun stop(): Boolean {
        val t = thread ?: return false
        thread = null
        running = false
        watch?.close(); watch = null
        val ar = record
        routing?.let { l -> quietly("remove routing") { ar?.removeOnRoutingChangedListener(l) } }
        routing = null
        quietly("stop capture") { ar?.stop() } // unblocks read()
        quietly("join") { t.join(5_000) }
        deliver()
        quietly("release encoder") { codec?.stop(); codec?.release() }
        quietly("release capture") { ar?.release() }
        pipeline?.let { base = it.reached }
        record = null; codec = null; pipeline = null
        LiveClock.current = null
        return finished
    }

    private inline fun quietly(what: String, block: () -> Unit) {
        try { block() } catch (t: Throwable) { Log.w(TAG, "$what: ${t.javaClass.simpleName}") }
    }

    companion object {
        /** `meeting-…` from `…/meeting-…-NN.m4a`. */
        fun meetingId(segmentPath: String) = File(segmentPath).name.substringBeforeLast('-')
    }
}
