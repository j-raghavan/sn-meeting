package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.MicChoice
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

    @Test fun rememberedChoiceWinsWhenPresent() { // F2-FR8
        val p = MicSelector.pick(usb, jabra, MicChoice.Bluetooth("Jabra"))
        assertEquals("Bluetooth — Jabra", p.chosen!!.label)
        assertEquals(MicChoice.Usb("USB-Audio - USB Audio"), p.toggled())
    }

    @Test fun toggleSwitchesBetweenTheTwo() {
        val p = MicSelector.pick(usb, jabra, MicChoice.Auto)
        assertEquals(MicChoice.Bluetooth("Jabra"), p.toggled())
        assertNull(MicSelector.pick(usb, BtSnapshot.NONE).toggled()) // one mic: nothing to switch to
        assertNull(MicSelector.pick(emptyList(), BtSnapshot.NONE).toggled())
    }

    @Test fun rememberedMicAbsentFallsBackToUsbFirst() {
        assertEquals(MicKind.USB, MicSelector.pick(usb, BtSnapshot.NONE, MicChoice.Bluetooth("Jabra")).chosen!!.kind)
        val two = jabra.copy(hfpConnected = listOf("Jabra", "Bose"))
        assertEquals("Bluetooth — Jabra", MicSelector.pick(emptyList(), two, MicChoice.Usb("Gone")).chosen!!.label)
    }

    @Test fun nameMatchIsExact() {
        assertEquals("USB — USB-Audio - USB Audio", MicSelector.pick(usb, jabra, MicChoice.Bluetooth("jabra")).chosen!!.label)
        assertEquals("Bluetooth — Bose", MicSelector.pick(emptyList(), jabra.copy(hfpConnected = listOf("Jabra", "Bose")), MicChoice.Bluetooth("Bose")).chosen!!.label)
    }

    @Test fun bluetoothOffNamesTheRememberedHeadset() { // EC11: bonded list is empty while off
        assertEquals("Turn on Bluetooth to use Jabra", MicSelector.pick(emptyList(), BtSnapshot.NONE, MicChoice.Bluetooth("Jabra")).reason)
        assertEquals(MicSelector.NONE, MicSelector.pick(emptyList(), BtSnapshot.NONE, MicChoice.Usb("x")).reason)
    }
}
