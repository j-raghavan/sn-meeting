package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.MicChoice

enum class MicKind(val wire: String, val label: String) { USB("usb", "USB"), BLUETOOTH("bluetooth", "Bluetooth") }

/** A mic that can record now. [deviceId] is the AudioDeviceInfo id for USB (Bluetooth gets one once SCO is up). */
data class MicOption(val kind: MicKind, val name: String, val deviceId: Int?) {
    val label get() = "${kind.label} — $name"
    val choice: MicChoice get() = if (kind == MicKind.USB) MicChoice.Usb(name) else MicChoice.Bluetooth(name)

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

    /** Tapping the mic card (F2-FR8): the next option after the chosen one, when there are two or more. */
    fun toggled(): MicChoice? = if (options.size < 2 || chosen == null) null
        else options[(options.indexOf(chosen) + 1) % options.size].choice
}

/** F2-FR2 / F2-FR8 / EC11 / EC12. Pure: the Android side only gathers [UsbInput]s and a [BtSnapshot]. */
object MicSelector {
    const val NONE = "Connect a USB or Bluetooth microphone"

    /**
     * The remembered [choice] (matched by kind and exact device name) whenever it is available,
     * else USB first, then Bluetooth. A remembered mic that is absent is kept in settings, not
     * forgotten: it wins again when it comes back.
     */
    fun pick(usb: List<UsbInput>, bt: BtSnapshot, choice: MicChoice = MicChoice.Auto): MicPick {
        val options = usb.map { MicOption(MicKind.USB, it.name, it.deviceId) } +
            (if (bt.adapterOn && bt.scoAvailableOffCall) bt.hfpConnected.map { MicOption(MicKind.BLUETOOTH, it, null) } else emptyList())
        val chosen = options.firstOrNull { it.choice == choice } ?: options.firstOrNull() // USB first: listed first
        return MicPick(options, chosen, if (chosen == null) reason(bt, choice) else null)
    }

    /** Why nothing can record, as specifically as the snapshot allows. */
    private fun reason(bt: BtSnapshot, choice: MicChoice): String {
        if (!bt.adapterOn) {
            // getBondedDevices() is empty while Bluetooth is off, so fall back to the remembered headset.
            val name = bt.bondedAudio.firstOrNull() ?: (choice as? MicChoice.Bluetooth)?.name
            if (name != null) return "Turn on Bluetooth to use $name"
        }
        bt.a2dpOnly.firstOrNull()?.let { return "$it isn't available as a microphone" }
        if (bt.hfpConnected.isNotEmpty() && !bt.scoAvailableOffCall) return "${bt.hfpConnected.first()} isn't available as a microphone"
        return NONE
    }
}
