package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.MicChoice
import dev.jraghavan.snmeeting.model.SegmentAdded
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** B2: Stop in every phase before audio is written cancels; nothing starts recording later. */
class RecordingSessionTest {
    private val log = mutableListOf<String>()
    private val published = mutableListOf<RecState>()
    private var ended: RecState.Idle? = null
    private val plan = Begun("m", "/s/m-01.m4a", 900_000, 13_499_997)
    private var pendingBegin: ((CoreResult<Begun>) -> Unit)? = null
    private var deferBegin = false
    private val timers = mutableListOf<() -> Unit>()
    private var btReady = false
    private var bt = BtSnapshot.NONE
    private var usb = listOf(UsbInput(7, "USB-Audio"))
    private var scoInput: Int? = null
    private var onBtChange: (() -> Unit)? = null
    private lateinit var link: ScoLink
    private lateinit var events: RecorderEvents
    private var stopThrows = false

    private val core = object : RecorderCore {
        override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) {
            log += "core begin"
            if (deferBegin) pendingBegin = done else done(CoreResult.Ok(plan))
        }
        override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {}
        override fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long) { log += "core close $index" }
        override fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit) {
            log += "core finish discard=$discard"; done()
        }
    }
    private val rec = object : RecorderPort {
        override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int) = true
        override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) {
            log += "mediarecorder start dev=$deviceId"
            this@RecordingSessionTest.events = events
        }
        override fun setNext(path: String) = true
        override fun stop(): Boolean {
            log += "mediarecorder stop"
            if (stopThrows) throw IllegalStateException()
            return true
        }
    }
    private val storage = object : StoragePort {
        override fun usableBytes(path: String) = Long.MAX_VALUE / 4
        override fun delete(path: String) {}
    }
    private val clock = object : ClockPort { override fun elapsedMs() = 0L; override fun wallMs() = 0L }
    private val audio = object : ScoAudio {
        override fun setCommunicationMode(on: Boolean) { log += if (on) "mode:comm" else "mode:normal" }
        override fun startSco() { log += "sco:start" }
        override fun stopSco() { log += "sco:stop" }
        override fun scoInputId() = scoInput
    }
    private val timer = TimerPort { ms, r -> timers += r; Cancel { timers -= r; log += "timer cancel $ms" } }

    private val env = object : RecordingSession.Env {
        override val timer = this@RecordingSessionTest.timer
        override fun usbInputs() = usb
        override fun openBluetooth(onChange: () -> Unit): BtPort {
            log += "bt open"; onBtChange = onChange
            return object : BtPort {
                override val ready get() = btReady
                override fun snapshot() = bt
                override fun close() { log += "bt close" }
            }
        }
        override fun newRecorder(listener: SegmentedRecorder.Listener) = SegmentedRecorder(core, rec, storage, clock, listener)
        override fun newSco(listener: ScoLink.Listener) = ScoLink(audio, timer, listener).also { link = it }
        override fun wake() = object : WakePort {
            override fun acquire(timeoutMs: Long) {}
            override fun release() { log += "wake release" }
        }
        override fun watchDevices() = Cancel { log += "devices off" }
        override fun watchSco() = Cancel { log += "sco receiver off" }
        override fun publish(s: RecState) { published += s }
        override fun showRecording(label: String, startElapsedMs: Long) { log += "notify $label" }
        override fun ended(end: RecState.Idle) { ended = end }
    }
    private val session = RecordingSession(env)
    private val headset = BtSnapshot(true, listOf("Jabra"), emptyList(), listOf("Jabra"), true)

    @Test fun stopWhileWaitingForBluetoothCancels() {
        session.begin(MicChoice.Auto)
        assertEquals(1, timers.size)
        session.stop()
        assertEquals(RecState.Idle(RecordingSession.CANCELLED, null), ended)
        assertEquals(RecState.Idle(RecordingSession.CANCELLED, null), published.last())
        assertTrue(log.containsAll(listOf("timer cancel 1500", "bt close")))
        // The proxy arriving now starts nothing.
        btReady = true; onBtChange!!()
        assertTrue(log.none { it.startsWith("core") || it.startsWith("mediarecorder") })
        assertEquals(RecordingSession.Phase.ENDED, session.phase)
    }

    @Test fun stopWhileScoConnectsClosesTheLinkAndNeverRecords() {
        btReady = true; bt = headset; usb = emptyList()
        session.begin(MicChoice.Auto)
        assertEquals(RecordingSession.Phase.CONNECTING, session.phase)
        session.stop()
        assertEquals(RecState.Idle(RecordingSession.CANCELLED, null), ended)
        assertTrue(log.containsAll(listOf("sco:stop", "mode:normal", "sco receiver off", "devices off", "wake release", "bt close")))
        // SCO coming up afterwards is ignored: the link is closed.
        scoInput = 42
        link.onScoState(ScoState.CONNECTING); link.onScoState(ScoState.CONNECTED); link.onInputsChanged()
        session.onScoState(ScoState.CONNECTED)
        assertTrue(log.none { it.startsWith("core") || it.startsWith("mediarecorder") })
        assertEquals(ScoLink.Phase.CLOSED, link.phase)
    }

    @Test fun stopWhileTheCoreBeginsDiscardsTheMeeting() {
        btReady = true; deferBegin = true
        session.begin(MicChoice.Auto)
        assertEquals(listOf("bt open", "core begin"), log)
        session.stop()
        assertEquals(RecState.Idle(RecordingSession.CANCELLED, null), ended)
        log.clear()
        pendingBegin!!(CoreResult.Ok(plan))
        assertEquals(listOf("core close 0", "core finish discard=true"), log)
    }

    @Test fun aUsbRecordingStartsAndStopsSaved() {
        btReady = true
        session.begin(MicChoice.Auto)
        assertEquals(RecState.Recording("m", "USB — USB-Audio", 0), published.last())
        assertEquals(7, session.pinnedId)
        session.onDevicesRemoved(listOf(3)) // another input: nothing
        assertEquals(RecordingSession.Phase.RECORDER, session.phase)
        session.stop()
        assertEquals(RecState.Idle("Saved", "m"), ended)
        assertTrue(published.contains(RecState.Stopping))
    }

    @Test fun theBluetoothWaitThenPicksAndNoMicEnds() {
        usb = emptyList()
        session.begin(MicChoice.Auto)
        timers.single()()
        assertEquals(RecState.Idle(MicSelector.NONE, null), ended)
        session.stop() // after the end: nothing
        assertEquals(1, published.count { it is RecState.Idle })
    }

    @Test fun aHeadsetRecordsOnceScoIsReadyAndLosingThePinnedInputEnds() {
        btReady = true; bt = headset; usb = emptyList()
        session.begin(MicChoice.Auto)
        scoInput = 42
        session.onScoState(ScoState.CONNECTING); session.onScoState(ScoState.CONNECTED)
        assertTrue(log.contains("mediarecorder start dev=42"))
        session.onDevicesAdded()
        session.onDevicesRemoved(listOf(42))
        assertEquals(RecState.Idle(EndReason.MIC_LOST.message, "m"), ended)
    }

    @Test fun aHeadsetThatDisconnectsWhileConnectingFails() {
        btReady = true; bt = headset; usb = emptyList()
        session.begin(MicChoice.Auto)
        session.onHeadsetDisconnected()
        assertEquals(RecState.Idle(ScoLink.DIDNT_CONNECT, null), ended)
    }

    @Test fun destroyFinalisesAndReleases() {
        btReady = true
        session.begin(MicChoice.Auto)
        session.destroy()
        assertTrue(log.containsAll(listOf("mediarecorder stop", "core finish discard=false", "devices off", "bt close")))
        session.begin(MicChoice.Auto) // a second begin on an ended session does nothing
        assertEquals(1, log.count { it == "core begin" })
    }

    @Test fun stopWithNothingRunningJustEnds() {
        session.stop()
        assertEquals(RecState.Idle(null, null), ended)
    }

    private fun recordUsb() { btReady = true; session.begin(MicChoice.Auto); log.clear() }

    @Test fun routingThenDeviceRemovalEndsOnce() { // F2-FR3: whichever fires first ends it
        recordUsb()
        events.onMicLost()
        session.onDevicesRemoved(listOf(7))
        assertEquals(RecState.Idle(EndReason.MIC_LOST.message, "m"), ended)
        assertEquals(1, log.count { it == "core finish discard=false" })
        assertEquals(1, log.count { it == "mediarecorder stop" })
    }

    @Test fun deviceRemovalThenRoutingEndsOnce() {
        recordUsb()
        session.onDevicesRemoved(listOf(7))
        events.onRoutedTo(3); events.onMicLost(); events.onFault("routing", IllegalStateException())
        assertEquals(RecState.Idle(EndReason.MIC_LOST.message, "m"), ended)
        assertEquals(1, log.count { it == "core finish discard=false" })
        assertEquals(1, published.count { it is RecState.Idle })
    }

    @Test fun aRecorderThatThrowsAsTheMicIsUnpluggedEndsSavedWithoutCrashing() { // the on-device crash
        recordUsb(); stopThrows = true
        events.onMicLost()
        session.onDevicesRemoved(listOf(7))
        assertEquals(RecState.Idle(EndReason.MIC_LOST.message, "m"), ended)
        assertEquals(listOf("mediarecorder stop", "core close 0", "core finish discard=false"), log.take(3))
        assertEquals(1, log.count { it == "core finish discard=false" })
        assertTrue(log.containsAll(listOf("devices off", "wake release", "bt close")))
    }

    @Test fun aFaultOnTheRecorderThreadEndsTheRecordingAsARecorderError() {
        recordUsb()
        session.onFault(IllegalStateException())
        session.onFault(IllegalStateException()) // once only
        assertEquals(RecState.Idle(EndReason.RECORDER_ERROR.message, "m"), ended)
        assertEquals(1, log.count { it == "core finish discard=false" })
    }

    @Test fun aFaultBeforeRecordingCancelsAndReleases() {
        session.begin(MicChoice.Auto) // waiting for Bluetooth
        session.onFault(IllegalStateException())
        assertEquals(RecState.Idle(EndReason.RECORDER_ERROR.message, null), ended)
        assertTrue(log.containsAll(listOf("timer cancel 1500", "bt close")))
        session.onFault(IllegalStateException())
        assertEquals(1, published.count { it is RecState.Idle })
    }

}
