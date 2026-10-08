package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.CoreJson

enum class MicKind(val wire: String, val label: String) { USB("usb", "USB"), BLUETOOTH("bluetooth", "Bluetooth") }

/** A mic that can record now. [deviceId] is the AudioDeviceInfo id for USB (Bluetooth gets one once SCO is up). */
data class MicOption(val kind: MicKind, val name: String, val deviceId: Int?) {
    val label get() = "${kind.label} — $name"

    /** The sidecar's `mic` (nativeBeginMeeting's micJson). */
    fun json() = """{"kind":"${kind.wire}","name":${CoreJson.quote(name)}}"""
}

data class UsbInput(val deviceId: Int, val name: String)

/** Bluetooth as the adapter sees it. Names only; no scan, no location (F2-FR10). */
data class BtSnapshot(
    val adapterOn: Boolean,
    /** Connected over HFP (BluetoothHeadset.getConnectedDevices). */
    val hfpConnected: List<String>,
    /** Connected over A2DP but not HFP (EC12). */
    val a2dpOnly: List<String>,
    /** Bonded audio-class devices; empty whenever the adapter is off (platform rule). */
    val bondedAudio: List<String>,
    val scoAvailableOffCall: Boolean,
) {
    companion object { val NONE = BtSnapshot(false, emptyList(), emptyList(), emptyList(), false) }
}

data class MicPick(val options: List<MicOption>, val chosen: MicOption?, val reason: String?) {
    val canRecord get() = chosen != null
}

/** F2-FR2 / EC11 / EC12. Pure: the Android side only gathers [UsbInput]s and a [BtSnapshot]. */
object MicSelector {
    const val NONE = "Connect a USB or Bluetooth microphone"

    fun pick(usb: List<UsbInput>, bt: BtSnapshot): MicPick {
        val options = usb.map { MicOption(MicKind.USB, it.name, it.deviceId) } +
            (if (bt.adapterOn && bt.scoAvailableOffCall) bt.hfpConnected.map { MicOption(MicKind.BLUETOOTH, it, null) } else emptyList())
        val chosen = options.firstOrNull() // USB first: listed first
        return MicPick(options, chosen, if (chosen == null) reason(bt) else null)
    }

    /** Why nothing can record, as specifically as the snapshot allows. */
    private fun reason(bt: BtSnapshot): String {
        if (!bt.adapterOn) bt.bondedAudio.firstOrNull()?.let { return "Turn on Bluetooth to use $it" }
        bt.a2dpOnly.firstOrNull()?.let { return "$it isn't available as a microphone" }
        if (bt.hfpConnected.isNotEmpty() && !bt.scoAvailableOffCall) return "${bt.hfpConnected.first()} isn't available as a microphone"
        return NONE
    }
}
