package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SegmentAdded
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** INV1 / S14: the recorder uses the pinned input or nothing. */
class PinTest {
    @Test fun anInputThatIsGoneOrRefusedCantBePinned() {
        Pin.require(found = true, preferred = true)
        val gone = assertThrows(IllegalStateException::class.java) { Pin.require(found = false, preferred = false) }
        assertEquals("the pinned microphone is gone", gone.message)
        val refused = assertThrows(IllegalStateException::class.java) { Pin.require(found = true, preferred = false) }
        assertEquals("the recorder refused the pinned microphone", refused.message)
    }

    @Test fun routedAnywhereElseIsMicLoss() {
        assertFalse(Pin.routedAway(pinned = 7, routed = 7))
        assertTrue(Pin.routedAway(pinned = 7, routed = 3))
        assertFalse("not routed yet: the routing callback reports it", Pin.routedAway(pinned = 7, routed = null))
    }

    @Test fun aRefusedPinIsARecorderThatWontStart() {
        // Through the port: start() throws, the recorder discards the meeting (F2-FR4).
        val log = mutableListOf<String>()
        var ended: String? = null
        val core = object : RecorderCore {
            override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) =
                done(CoreResult.Ok(Begun("m", "/s/m-01.m4a", 900_000, 13_499_997)))
            override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {}
            override fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long) { log += "close" }
            override fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit) { log += "finish discard=$discard"; done() }
        }
        val rec = object : RecorderPort {
            override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int) = true
            override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) =
                Pin.require(found = true, preferred = false)
            override fun setNext(path: String) = true
            override fun stop() = true
        }
        val storage = object : StoragePort { override fun usableBytes(path: String) = Long.MAX_VALUE / 4; override fun delete(path: String) {} }
        val clock = object : ClockPort { override fun elapsedMs() = 0L; override fun wallMs() = 0L }
        SegmentedRecorder(core, rec, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) { log += "recording" }
            override fun onSegmentStarted(index: Int) {}
            override fun onEnded(id: String?, message: String) { ended = message }
        }).start(MicOption(MicKind.USB, "USB", 7), 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(listOf("close", "finish discard=true"), log)
        assertEquals(SegmentedRecorder.CANT_START, ended)
    }
}
