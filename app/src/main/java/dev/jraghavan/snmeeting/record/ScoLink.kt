package dev.jraghavan.snmeeting.record

/** What BtScoLink needs from AudioManager (DEVICE-UNVERIFIED adapter: AndroidScoAudio). */
interface ScoAudio {
    fun setCommunicationMode(on: Boolean) // MODE_IN_COMMUNICATION / MODE_NORMAL
    fun startSco()
    fun stopSco()
    /** The TYPE_BLUETOOTH_SCO input's id in getDevices(GET_DEVICES_INPUTS), or null. */
    fun scoInputId(): Int?
}

/** AudioManager.SCO_AUDIO_STATE_* values. */
object ScoState { const val ERROR = -1; const val DISCONNECTED = 0; const val CONNECTED = 1; const val CONNECTING = 2 }

/**
 * F2-FR9's SCO lifecycle as a state machine. All calls on the recording thread. The stop
 * sequence (stopBluetoothSco, then MODE_NORMAL) runs once on every exit, and only for what was
 * actually started (INV7). See the design's SCO table.
 */
class ScoLink(
    private val audio: ScoAudio,
    private val timer: TimerPort,
    private val listener: Listener,
    private val timeoutMs: Long = 5_000,
) {
    interface Listener {
        fun onReady(deviceId: Int)
        fun onFailed(reason: String)
        fun onLost()
    }

    enum class Phase { IDLE, CONNECTING, READY, CLOSED }

    var phase = Phase.IDLE
        private set
    private var modeSet = false
    private var scoStarted = false
    private var scoUp = false
    private var sawConnecting = false
    private var deadline: Cancel? = null

    /** Register the SCO receiver and device callback BEFORE calling this (the sticky broadcast). */
    fun open() {
        check(phase == Phase.IDLE) { "open twice" }
        phase = Phase.CONNECTING
        try {
            audio.setCommunicationMode(true); modeSet = true
            audio.startSco(); scoStarted = true
        } catch (e: Exception) {
            fail(DIDNT_CONNECT); return
        }
        deadline = timer.after(timeoutMs) { if (phase == Phase.CONNECTING) fail(DIDNT_CONNECT) }
        checkReady()
    }

    fun onScoState(state: Int) {
        when (phase) {
            Phase.CONNECTING -> when (state) {
                ScoState.CONNECTING -> sawConnecting = true
                ScoState.CONNECTED -> { scoUp = true; checkReady() }
                // The sticky intent replays the old DISCONNECTED on register: ignore it until
                // the link has at least started connecting.
                ScoState.DISCONNECTED, ScoState.ERROR -> if (sawConnecting || scoUp) fail(DIDNT_CONNECT)
            }
            Phase.READY -> if (state == ScoState.DISCONNECTED || state == ScoState.ERROR) lose()
            else -> Unit
        }
    }

    /** AudioDeviceCallback, either direction. */
    fun onInputsChanged() {
        when (phase) {
            Phase.CONNECTING -> checkReady()
            Phase.READY -> if (audio.scoInputId() == null) lose()
            else -> Unit
        }
    }

    /** BluetoothHeadset ACTION_CONNECTION_STATE_CHANGED → disconnected. */
    fun onHeadsetDisconnected() {
        when (phase) {
            Phase.CONNECTING -> fail(DIDNT_CONNECT)
            Phase.READY -> lose()
            else -> Unit
        }
    }

    /** The stop sequence. Idempotent; safe from any phase (normal stop, error, onDestroy). */
    fun close() {
        if (phase == Phase.CLOSED) return
        phase = Phase.CLOSED
        deadline?.cancel(); deadline = null
        if (scoStarted) { scoStarted = false; runCatching { audio.stopSco() } }
        if (modeSet) { modeSet = false; runCatching { audio.setCommunicationMode(false) } }
    }

    private fun checkReady() {
        if (phase != Phase.CONNECTING || !scoUp) return
        val id = audio.scoInputId() ?: return // the input often appears just after the broadcast
        phase = Phase.READY
        deadline?.cancel(); deadline = null
        listener.onReady(id)
    }

    private fun fail(reason: String) { close(); listener.onFailed(reason) }
    private fun lose() { close(); listener.onLost() }

    companion object { const val DIDNT_CONNECT = "Bluetooth mic didn't connect" }
}
