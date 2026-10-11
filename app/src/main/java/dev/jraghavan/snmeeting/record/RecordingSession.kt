package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.MicChoice

/** Bluetooth as the session sees it (BluetoothState on the device). */
interface BtPort {
    /** The headset proxy is bound (or never will be), so [snapshot] can list HFP headsets. */
    val ready: Boolean
    fun snapshot(): BtSnapshot
    fun close()
}

/**
 * F2-FR1, F2-FR9: one recording from ＋ New to its end, as plain Kotlin over ports, on the
 * recording thread. RecordingService is the platform adapter around it. A Stop before audio is
 * being written (waiting for the headset proxy, SCO connecting, the core's begin in flight)
 * cancels: the SCO link is closed so it never reports ready, a begun meeting is finished and
 * discarded, and the UI reads "Recording cancelled".
 */
class RecordingSession(private val env: Env, private val btWaitMs: Long = BT_WAIT_MS) {
    interface Env {
        val timer: TimerPort
        fun usbInputs(): List<UsbInput>
        /** F22-FR1: the device's own mic, if it has one. */
        fun builtInInputs(): List<UsbInput> = emptyList()
        /** F22-FR3: the platform records UNPROCESSED audio here. */
        val unprocessedSupported: Boolean get() = false
        /** Opens Bluetooth; [onChange] runs on the recording thread when a proxy or state changes. */
        fun openBluetooth(onChange: () -> Unit): BtPort
        fun newRecorder(listener: SegmentedRecorder.Listener): SegmentedRecorder
        fun newSco(listener: ScoLink.Listener): ScoLink
        fun wake(): WakePort
        /** Registers the audio-device callback; the handle unregisters it. */
        fun watchDevices(): Cancel
        /** Registers the SCO and headset broadcasts (before ScoLink.open: the sticky replay). */
        fun watchSco(): Cancel
        fun publish(s: RecState)
        /** The ongoing notification once audio is being written. */
        fun showRecording(label: String, startElapsedMs: Long)
        /** Everything is released: leave the foreground and stop the service. */
        fun ended(end: RecState.Idle)
    }

    enum class Phase { IDLE, PICKING, CONNECTING, RECORDER, ENDED }

    var phase = Phase.IDLE
        private set
    private val guard = ExitGuard()
    private var bt: BtPort? = null
    private var btWait: Cancel? = null
    private var recorder: SegmentedRecorder? = null
    private var sco: ScoLink? = null
    /** The input the recording is pinned to (F2-FR3). */
    var pinnedId: Int? = null
        private set

    /**
     * Opens Bluetooth and picks the mic once the headset proxy is bound (it arrives
     * asynchronously), or after [btWaitMs]: a USB mic or a headset bound by then is offered.
     */
    fun begin(choice: MicChoice) {
        if (phase != Phase.IDLE) return
        phase = Phase.PICKING
        val state = env.openBluetooth { if (bt?.ready == true) go(choice) }
        bt = state
        guard.hold("bluetooth") { state.close() }
        if (state.ready) go(choice) else btWait = env.timer.after(btWaitMs) { go(choice) }
    }

    private fun go(choice: MicChoice) {
        if (phase != Phase.PICKING) return
        btWait?.cancel(); btWait = null
        val pick = MicSelector.pick(env.usbInputs(), bt?.snapshot() ?: BtSnapshot.NONE, choice, env.builtInInputs())
        val mic = pick.chosen ?: return end(RecState.Idle(pick.reason, null))
        env.publish(RecState.Starting(mic.label))
        val wake = WakeLease(env.wake()).also { it.begin(guard) } // F2-FR6: the screen may sleep
        val rec = env.newRecorder(object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) {
                env.publish(RecState.Recording(id, mic.label, startElapsedMs))
                env.showRecording(mic.label, startElapsedMs)
            }
            override fun onSegmentStarted(index: Int) = wake.renew()
            override fun onEnded(id: String?, message: String) = end(RecState.Idle(message, id))
        })
        recorder = rec
        val devices = env.watchDevices()
        guard.hold("deviceCallback") { devices.cancel() }
        if (mic.kind == MicKind.USB) {
            val id = mic.deviceId ?: return end(RecState.Idle(MicSelector.NONE, null))
            pinnedId = id
            phase = Phase.RECORDER
            rec.start(mic, id, InputSource.MIC, listOf(AudioSpec.USB))
            return
        }
        if (mic.kind == MicKind.BUILT_IN) {
            // F22-FR2: pinned like USB (INV1); F22-FR3: the best source the recorder accepts.
            val id = mic.deviceId ?: return end(RecState.Idle(MicSelector.NONE, null))
            pinnedId = id
            phase = Phase.RECORDER
            rec.start(mic, id, builtInSources(env.unprocessedSupported), AudioSpec.BUILT_IN)
            return
        }
        // F2-FR9: Bluetooth records only once SCO is up; nativeBeginMeeting waits for onReady.
        phase = Phase.CONNECTING
        val link = env.newSco(object : ScoLink.Listener {
            override fun onReady(deviceId: Int) {
                if (phase != Phase.CONNECTING) return
                pinnedId = deviceId
                phase = Phase.RECORDER
                rec.start(mic, deviceId, InputSource.VOICE_COMMUNICATION, listOf(AudioSpec.BLUETOOTH, AudioSpec.BLUETOOTH_FALLBACK))
            }
            override fun onFailed(reason: String) = end(RecState.Idle(reason, null))
            override fun onLost() = rec.onMicLost()
        })
        sco = link
        val broadcasts = env.watchSco()
        guard.hold("scoReceiver") { broadcasts.cancel() }
        guard.hold("sco") { link.close() } // stopBluetoothSco, then MODE_NORMAL (INV7)
        link.open()
    }

    /** User Stop, notification Stop. */
    fun stop() {
        when (phase) {
            Phase.IDLE -> end(RecState.Idle(null, null))
            Phase.PICKING, Phase.CONNECTING -> {
                sco?.close() // never reports ready now
                recorder?.stop() // not started: it ends, and a late start does nothing
                end(RecState.Idle(CANCELLED, null))
            }
            // A begin in flight ends as cancelled and discards the meeting when the core replies.
            Phase.RECORDER -> { env.publish(RecState.Stopping); recorder?.stop() }
            Phase.ENDED -> Unit
        }
    }

    /**
     * Something on the recorder thread threw (Safely, or the thread's last-resort handler): a
     * recording under way ends as a recorder error with its segment finalised and finished once;
     * before that, everything is released. Ending twice is a no-op.
     */
    fun onFault(t: Throwable) {
        when (phase) {
            Phase.RECORDER -> recorder?.onFault("recorder thread", t) ?: end(RecState.Idle(EndReason.RECORDER_ERROR.message, null))
            Phase.IDLE, Phase.PICKING, Phase.CONNECTING -> {
                try { sco?.close() } catch (_: Throwable) {} // never reports ready now
                recorder?.stop() // not started: it ends, and a late start does nothing
                end(RecState.Idle(EndReason.RECORDER_ERROR.message, null))
            }
            Phase.ENDED -> Unit
        }
    }

    /** The service is being destroyed by the system: finalise what we can, release (INV7). */
    fun destroy() {
        recorder?.stop()
        btWait?.cancel(); btWait = null
        guard.releaseAll()
    }

    /** F2-FR3, EC1, EC2: the pinned input going away ends the recording; nothing else is used. */
    fun onDevicesRemoved(ids: List<Int>) {
        if (pinnedId != null && pinnedId in ids) recorder?.onMicLost()
        sco?.onInputsChanged()
    }

    fun onDevicesAdded() { sco?.onInputsChanged() }

    /** EC10: SCO dropping or the headset disconnecting (out of range, battery). */
    fun onScoState(state: Int) { sco?.onScoState(state) }

    fun onHeadsetDisconnected() { sco?.onHeadsetDisconnected() }

    /** The one exit: release everything, publish, then let the service stop. */
    private fun end(idle: RecState.Idle) {
        if (phase == Phase.ENDED) return
        phase = Phase.ENDED
        btWait?.cancel(); btWait = null
        guard.releaseAll()
        recorder = null; sco = null; bt = null; pinnedId = null
        env.publish(idle)
        env.ended(idle)
    }

    companion object {
        const val BT_WAIT_MS = 1_500L
        const val CANCELLED = "Recording cancelled"
    }
}
