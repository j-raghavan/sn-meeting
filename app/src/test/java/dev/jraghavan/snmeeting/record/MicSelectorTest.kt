package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class MicSelectorTest {
    private val usb = listOf(UsbInput(7, "USB-Audio - USB Audio"))
    private val jabra = BtSnapshot(true, listOf("Jabra"), emptyList(), listOf("Jabra"), true)

    @Test fun usbFirstThenBluetooth() {
        val p = MicSelector.pick(usb, jabra)
        assertEquals("USB — USB-Audio - USB Audio", p.chosen!!.label)
        assertEquals(listOf(MicKind.USB, MicKind.BLUETOOTH), p.options.map { it.kind })
        assertEquals("Bluetooth — Jabra", MicSelector.pick(emptyList(), jabra).chosen!!.label)
        assertEquals("""{"kind":"bluetooth","name":"Jabra"}""", MicSelector.pick(emptyList(), jabra).chosen!!.json())
    }

    @Test fun nothingConnected() {
        val p = MicSelector.pick(emptyList(), BtSnapshot.NONE)
        assertFalse(p.canRecord)
        assertEquals(MicSelector.NONE, p.reason)
    }

    @Test fun bluetoothOffNamesThePairedHeadset() { // EC11
        val off = BtSnapshot(false, emptyList(), emptyList(), listOf("Jabra"), false)
        assertEquals("Turn on Bluetooth to use Jabra", MicSelector.pick(emptyList(), off).reason)
    }

    @Test fun musicOnlyHeadsetIsNotAMic() { // EC12
        val music = BtSnapshot(true, emptyList(), listOf("Sony WH"), listOf("Sony WH"), true)
        assertEquals("Sony WH isn't available as a microphone", MicSelector.pick(emptyList(), music).reason)
    }

    @Test fun headsetWithoutScoOffCallIsNotAMic() {
        val p = MicSelector.pick(emptyList(), jabra.copy(scoAvailableOffCall = false))
        assertNull(p.chosen)
        assertEquals("Jabra isn't available as a microphone", p.reason)
    }

    @Test fun bluetoothNeedsTheAdapterOn() {
        assertNull(MicSelector.pick(emptyList(), jabra.copy(adapterOn = false)).chosen)
    }
}
