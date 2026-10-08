package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Test

class ScoLinkTest {
    private val log = mutableListOf<String>()
    private var scoInput: Int? = null
    private var fire: (() -> Unit)? = null
    private var failStart = false
    private val audio = object : ScoAudio {
        override fun setCommunicationMode(on: Boolean) { log += if (on) "mode:comm" else "mode:normal" }
        override fun startSco() { if (failStart) throw IllegalStateException(); log += "sco:start" }
        override fun stopSco() { log += "sco:stop" }
        override fun scoInputId() = scoInput
    }
    private val timer = TimerPort { ms, r -> log += "timer:$ms"; fire = r; Cancel { fire = null; log += "timer:cancel" } }
    private val link = ScoLink(audio, timer, object : ScoLink.Listener {
        override fun onReady(deviceId: Int) { log += "ready:$deviceId" }
        override fun onFailed(reason: String) { log += "failed:$reason" }
        override fun onLost() { log += "lost" }
    })

    @Test fun readyNeedsBothTheBroadcastAndTheInput() {
        link.open()
        link.onScoState(ScoState.DISCONNECTED) // sticky replay: ignored
        link.onScoState(ScoState.CONNECTING)
        link.onScoState(ScoState.CONNECTED)
        assertEquals(ScoLink.Phase.CONNECTING, link.phase)
        scoInput = 42; link.onInputsChanged()
        assertEquals(listOf("mode:comm", "sco:start", "timer:5000", "timer:cancel", "ready:42"), log)
        assertEquals(ScoLink.Phase.READY, link.phase)
    }

    @Test fun timeoutRunsTheStopSequenceOnce() { // EC13
        link.open(); fire!!(); link.close()
        assertEquals(listOf("mode:comm", "sco:start", "timer:5000", "timer:cancel", "sco:stop", "mode:normal",
            "failed:Bluetooth mic didn't connect"), log)
    }

    @Test fun dropWhileRecordingIsLossAndReleases() {
        scoInput = 3; link.open(); link.onScoState(ScoState.CONNECTED)
        link.onScoState(ScoState.DISCONNECTED)
        assertEquals(listOf("mode:comm", "sco:start", "timer:5000", "timer:cancel", "ready:3", "sco:stop", "mode:normal", "lost"), log)
    }

    @Test fun inputGoneWhileRecordingIsLoss() {
        scoInput = 3; link.open(); link.onScoState(ScoState.CONNECTED)
        scoInput = null; link.onInputsChanged()
        assertEquals("lost", log.last())
    }

    @Test fun headsetDisconnectWhileConnectingFails() {
        link.open(); link.onHeadsetDisconnected()
        assertEquals("failed:Bluetooth mic didn't connect", log.last())
        link.onScoState(ScoState.CONNECTED); link.onHeadsetDisconnected() // closed: ignored
        assertEquals(1, log.count { it.startsWith("failed") })
    }

    @Test fun errorAfterConnectingFails() {
        link.open(); link.onScoState(ScoState.CONNECTING); link.onScoState(ScoState.ERROR)
        assertEquals(listOf("sco:stop", "mode:normal", "failed:Bluetooth mic didn't connect"), log.takeLast(3))
    }

    @Test fun aStartThatThrowsRestoresTheModeOnly() {
        failStart = true; link.open()
        assertEquals(listOf("mode:comm", "mode:normal", "failed:Bluetooth mic didn't connect"), log)
    }

    @Test fun closeBeforeOpenStopsNothing() {
        link.close(); link.close()
        assertEquals(emptyList<String>(), log)
    }
}
