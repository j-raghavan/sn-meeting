package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SegmentAdded
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SegmentedRecorderTest {
    private val log = mutableListOf<String>()
    private var now = 1_000L
    private var free = 10_000_000_000L
    private var stopOk = true
    private var nextOk = true
    private var startFails = false
    private var beginReply: CoreResult<Begun>? = null
    private val rejected = mutableSetOf<AudioSpec>()
    private lateinit var events: RecorderEvents
    private val id = "meeting-20261008-100000"
    private val first = "/s/EXPORT/sn-meeting/$id-01.m4a"
    private val plan = Begun(id, first, 900_000, 13_499_997)
    private var ended: Pair<String?, String>? = null

    private val core = object : RecorderCore {
        override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) {
            log += "begin $micJson $audioJson"; done(beginReply ?: CoreResult.Ok(plan))
        }
        override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {
            log += "add ${path.substringAfterLast('-')} @$startOffsetMs"; done(CoreResult.Ok(SegmentAdded(1, path)))
        }
        override fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long) {
            log += "close $index ${path.substringAfterLast('-')} measure=$measure ~$fallbackMs"
        }
        override fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit) {
            log += "finish $durationMs '$note' discard=$discard"; done()
        }
    }
    private val rec = object : RecorderPort {
        override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int) = spec !in rejected
        override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) {
            if (startFails) throw IllegalStateException("prepare failed")
            log += "start ${path.substringAfterLast('-')} $source dev=$deviceId bytes=$maxFileBytes ms=$maxDurationMs"
            this@SegmentedRecorderTest.events = events
        }
        override fun setNext(path: String): Boolean { log += "next ${path.substringAfterLast('-')} $nextOk"; return nextOk }
        override fun stop(): Boolean { log += "stop $stopOk"; return stopOk }
    }
    private val storage = object : StoragePort {
        override fun usableBytes(path: String) = free
        override fun delete(path: String) { log += "delete ${path.substringAfterLast('-')}" }
    }
    private val clock = object : ClockPort { override fun elapsedMs() = now; override fun wallMs() = 0L }
    private val sr = SegmentedRecorder(core, rec, storage, clock, object : SegmentedRecorder.Listener {
        override fun onRecording(id: String, startElapsedMs: Long) { log += "recording" }
        override fun onSegmentStarted(index: Int) {}
        override fun onEnded(id: String?, message: String) { ended = id to message }
    })
    private val usb = MicOption(MicKind.USB, "USB", 7)

    private fun startUsb() { sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB)); log.clear() }

    @Test fun usbBeginsWithItsSpecAndASizeCap() { // F2-FR4, F2-FR5
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals("""begin {"kind":"usb","name":"USB"} {"codec":"aac-lc","sampleRateHz":44100,"bitrateBps":96000,"channels":1}""", log[0])
        // 15 min at 96 kbps = 10.8 MB (< the plan's 13.5 MB cap); MAX_DURATION 10 % past the plan.
        assertEquals("start 01.m4a MIC dev=7 bytes=10800000 ms=990000", log[1])
        assertEquals(SegmentedRecorder.Phase.RECORDING, sr.phase)
    }

    @Test fun bluetoothFallsBackTo48k() { // F2-FR4
        rejected += AudioSpec.BLUETOOTH
        sr.start(MicOption(MicKind.BLUETOOTH, "Jabra", null), 9, InputSource.VOICE_COMMUNICATION, listOf(AudioSpec.BLUETOOTH, AudioSpec.BLUETOOTH_FALLBACK))
        assertEquals("""begin {"kind":"bluetooth","name":"Jabra"} {"codec":"aac-lc","sampleRateHz":16000,"bitrateBps":48000,"channels":1}""", log[0])
        assertEquals("start 01.m4a VOICE_COMMUNICATION dev=9 bytes=5400000 ms=990000", log[1])
    }

    @Test fun noAcceptedSpecBeginsNothing() {
        rejected += AudioSpec.USB
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(emptyList<String>(), log)
        assertEquals(null to SegmentedRecorder.CANT_START, ended)
    }

    @Test fun aProbeThatThrowsIsARecorderThatWontStart() { // S11: setAudioSource can throw
        val throwing = object : RecorderPort by rec {
            override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int): Boolean =
                throw RuntimeException("setAudioSource failed")
        }
        val r = SegmentedRecorder(core, throwing, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) { log += "recording" }
            override fun onSegmentStarted(index: Int) {}
            override fun onEnded(id: String?, message: String) { ended = id to message }
        })
        r.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(emptyList<String>(), log)
        assertEquals(null to SegmentedRecorder.CANT_START, ended)
    }

    @Test fun aRefusedBeginShowsTheCoreReason() {
        beginReply = CoreResult.Err("already recording meeting-x")
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(null to "already recording meeting-x", ended)
    }

    @Test fun aRecorderThatWontStartIsDiscarded() {
        startFails = true
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(listOf("close 0 01.m4a measure=false ~0", "finish 0 '' discard=true"), log.drop(1))
        assertEquals(null to SegmentedRecorder.CANT_START, ended)
    }

    @Test fun stopFinalisesWithTheMeasuredDuration() {
        startUsb(); now += 60_000; sr.stop(); sr.stop()
        assertEquals(listOf("stop true", "close 0 01.m4a measure=true ~60000", "finish 60000 '' discard=false"), log)
        assertEquals(id to "Saved", ended)
        assertEquals(SegmentedRecorder.Phase.ENDED, sr.phase)
    }

    @Test fun stopWhileBeginningLeavesNothingLive() {
        var pending: ((CoreResult<Begun>) -> Unit)? = null
        val slow = object : RecorderCore by core {
            override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) { pending = done }
        }
        val r = SegmentedRecorder(slow, rec, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) { log += "recording" }
            override fun onSegmentStarted(index: Int) {}
            override fun onEnded(id: String?, message: String) { log += "ended $message" }
        })
        r.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB)); r.stop()
        pending!!(CoreResult.Ok(plan))
        assertEquals(listOf("ended Recording cancelled", "close 0 01.m4a measure=false ~0", "finish 0 '' discard=true"), log)
    }

    @Test fun aStopBeforeStartEndsItAndStartThenDoesNothing() { // B2: Stop while SCO connects
        sr.stop()
        assertEquals(SegmentedRecorder.Phase.ENDED, sr.phase)
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(emptyList<String>(), log)
        assertEquals(null, ended)
        assertEquals(SegmentedRecorder.Phase.ENDED, sr.phase)
    }

    @Test fun segmentPathsSitBesideTheFirst() {
        assertEquals("/d/m-12.m4a", SegmentedRecorder.segmentPath("/d/m-01.m4a", "m", 12))
    }

    @Test fun gaplessRollover() { // F2-FR5
        startUsb()
        events.onApproachingLimit(); events.onApproachingLimit() // the second is ignored
        now += 900_050; events.onNextFileStarted()
        events.onNextFileStarted() // nothing armed: ignored
        now += 10_000; sr.stop()
        assertEquals(listOf("next 02.m4a true", "close 0 01.m4a measure=true ~900050", "add 02.m4a @900050",
            "stop true", "close 1 02.m4a measure=true ~10000", "finish 910050 '' discard=false"), log)
        assertEquals(id to "Saved", ended)
    }

    @Test fun fallbackStopsAndRestarts() { // R1
        nextOk = false; startUsb()
        events.onApproachingLimit()
        now += 990_000; events.onLimitReached()
        assertEquals(listOf("next 02.m4a false", "stop true", "close 0 01.m4a measure=true ~990000",
            "start 02.m4a MIC dev=7 bytes=10800000 ms=990000", "add 02.m4a @990000"), log)
    }

    @Test fun aFailedRestartEndsTheMeeting() {
        startUsb(); startFails = true
        now += 990_000; events.onLimitReached()
        assertEquals(listOf("stop true", "close 0 01.m4a measure=true ~990000", "finish 990000 'recorder error' discard=false"), log)
        assertEquals(id to "The recorder stopped — recording saved", ended)
    }

    @Test fun offsetsStayStrictlyIncreasing() {
        startUsb(); events.onApproachingLimit(); events.onNextFileStarted() // a clock that hasn't moved
        assertEquals("add 02.m4a @1", log.last())
    }

    @Test fun aSegmentTheCoreRefusesEndsTheMeeting() {
        val refusing = object : RecorderCore by core {
            override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {
                done(CoreResult.Err("segPath must be the next segment"))
            }
        }
        val r = SegmentedRecorder(refusing, rec, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) {}
            override fun onSegmentStarted(index: Int) {}
            override fun onEnded(id: String?, message: String) { ended = id to message }
        })
        r.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        now += 5_000; events.onApproachingLimit(); events.onNextFileStarted()
        assertEquals("finish 5000 'recorder error' discard=false", log.last())
    }

    @Test fun aSegmentPathTheCoreDisagreesWithEndsTheMeeting() { // N8: the core names the files
        val other = object : RecorderCore by core {
            override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {
                log += "add ${path.substringAfterLast('-')}"; done(CoreResult.Ok(SegmentAdded(1, "/elsewhere/x-02.m4a")))
            }
        }
        val r = SegmentedRecorder(other, rec, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) {}
            override fun onSegmentStarted(index: Int) {}
            override fun onEnded(id: String?, message: String) { ended = id to message }
        })
        r.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        now += 5_000; events.onApproachingLimit(); events.onNextFileStarted()
        assertEquals(id to EndReason.RECORDER_ERROR.message, ended)
        // The segment being written was closed by the path it was written to.
        assertTrue(log.any { it.startsWith("close 1 02.m4a") })
    }

    @Test fun eventsAfterTheEndAreIgnored() {
        startUsb(); sr.stop(); log.clear()
        events.onApproachingLimit(); events.onLimitReached(); events.onError(1)
        assertEquals(emptyList<String>(), log)
    }

    @Test fun noAudioOnTheOnlySegmentLeavesNothing() { // F2-FR7
        stopOk = false; startUsb(); sr.stop()
        assertEquals(listOf("stop false", "delete 01.m4a", "close 0 01.m4a measure=false ~0", "finish 0 '' discard=true"), log)
        assertEquals(null to "Too short — nothing saved", ended)
    }

    @Test fun noAudioInALaterSegmentKeepsTheMeeting() {
        startUsb(); events.onApproachingLimit(); now += 900_000; events.onNextFileStarted()
        stopOk = false; now += 100; sr.stop()
        assertEquals(listOf("stop false", "delete 02.m4a", "close 1 02.m4a measure=false ~0", "finish 900100 '' discard=false"), log.takeLast(4))
        assertEquals(id to "Saved", ended)
    }

    @Test fun micLossFinalisesWithTheNote() { // F2-FR3
        startUsb(); now += 5_000; sr.onMicLost()
        assertEquals(listOf("stop true", "close 0 01.m4a measure=true ~5000", "finish 5000 'mic disconnected' discard=false"), log)
        assertEquals(id to "Microphone disconnected — recording saved", ended)
        sr.onMicLost() // once only
        assertEquals(3, log.size)
    }

    @Test fun routedToAnotherInputIsMicLoss() { // INV1
        startUsb(); events.onRoutedTo(7); events.onRoutedTo(null)
        assertEquals(emptyList<String>(), log)
        events.onRoutedTo(1)
        assertEquals("finish 0 'mic disconnected' discard=false", log.last())
    }

    @Test fun storageFullAtTheLimit() { // EC5
        startUsb(); free = 5_000_000
        now += 800_000; events.onApproachingLimit(); events.onLimitReached()
        assertEquals(listOf("stop true", "close 0 01.m4a measure=true ~800000", "finish 800000 'storage full' discard=false"), log)
        assertEquals(id to "Storage full — recording saved", ended)
    }

    @Test fun aRecorderErrorWithNoRoomIsStorageFull() {
        startUsb(); now += 1_000; free = 0; events.onError(1)
        assertEquals("finish 1000 'storage full' discard=false", log.last())
    }

    @Test fun aRecorderErrorWithRoomIsARecorderError() {
        startUsb(); events.onError(100)
        assertEquals("finish 0 'recorder error' discard=false", log.last())
    }

    @Test fun noRoomToStartRecordsNothing() {
        free = 11_000_000 // under 10.8 MB + the 1 MB margin
        sr.start(usb, 7, InputSource.MIC, listOf(AudioSpec.USB))
        assertEquals(listOf("close 0 01.m4a measure=false ~0", "finish 0 '' discard=true"), log.drop(1))
        assertEquals(null to "Storage full", ended)
    }

    @Test fun onlyEndingsNobodyAskedForLeaveANotice() {
        assertEquals(listOf(EndReason.MIC_LOST, EndReason.STORAGE_FULL, EndReason.RECORDER_ERROR), EndReason.entries.filter { it.leavesNotice })
        assertEquals(EndReason.MIC_LOST, EndReason.ofMessage("Microphone disconnected — recording saved"))
        assertEquals(null, EndReason.ofMessage(SegmentedRecorder.TOO_SHORT))
    }
}
