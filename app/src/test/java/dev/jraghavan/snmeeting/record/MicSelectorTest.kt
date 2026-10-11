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

    @Test fun settingsListsAutomaticTheConnectedMicsAndTheRememberedOne() { // F10-FR2
        val options = MicSelector.pick(usb, jabra).options
        val rows = MicSelector.settingsRows(options, MicChoice.Auto)
        assertEquals(listOf("Automatic", "USB — USB-Audio - USB Audio", "Bluetooth — Jabra"), rows.map { it.label })
        assertEquals(listOf(true, false, false), rows.map { it.selected })
        val gone = MicSelector.settingsRows(options.take(1), MicChoice.Bluetooth("Jabra"))
        assertEquals("Bluetooth — Jabra", gone.last().label)
        assertEquals("Not connected now; used whenever it is", gone.last().hint)
        assertEquals(listOf(false, false, true), gone.map { it.selected })
        assertEquals(3, MicSelector.settingsRows(options, MicChoice.Usb("USB-Audio - USB Audio")).size)
    }

    private val builtIn = listOf(UsbInput(3, "Built-in mic"))

    @Test fun theBuiltInMicComesLastAndRecordsAlone() { // F22-FR1, F22-AC1, F22-AC2
        val all = MicSelector.pick(usb, jabra, MicChoice.Auto, builtIn)
        assertEquals(listOf(MicKind.USB, MicKind.BLUETOOTH, MicKind.BUILT_IN), all.options.map { it.kind })
        assertEquals(MicKind.USB, all.chosen!!.kind)
        val alone = MicSelector.pick(emptyList(), BtSnapshot.NONE, MicChoice.Auto, builtIn)
        assertEquals("Built-in — Built-in mic", alone.chosen!!.label)
        assertEquals(3, alone.chosen!!.deviceId)
        assertEquals("""{"kind":"builtin","name":"Built-in mic"}""", alone.chosen!!.json())
        assertNull(alone.reason)
        // A saved choice of the built-in mic wins over USB while it is there.
        assertEquals(MicKind.BUILT_IN, MicSelector.pick(usb, jabra, MicChoice.BuiltIn("Built-in mic"), builtIn).chosen!!.kind)
        assertEquals(MicChoice.BuiltIn("Built-in mic"), MicSelector.pick(usb, BtSnapshot.NONE, MicChoice.Auto, builtIn).toggled())
    }

    @Test fun settingsNameTheBuiltInMic() {
        val rows = MicSelector.settingsRows(MicSelector.pick(emptyList(), BtSnapshot.NONE, MicChoice.Auto, builtIn).options, MicChoice.Auto)
        assertEquals(listOf("Automatic", "Built-in — Built-in mic"), rows.map { it.label })
        assertEquals("USB first, then Bluetooth, then built-in", rows.first().hint)
        assertEquals("Built-in — Built-in mic", MicSelector.settingsRows(emptyList(), MicChoice.BuiltIn("Built-in mic")).last().label)
    }

    @Test fun theBuiltInSourcesInOrder() { // F22-FR3
        assertEquals(listOf(InputSource.UNPROCESSED, InputSource.VOICE_RECOGNITION, InputSource.MIC), builtInSources(true))
        assertEquals(listOf(InputSource.VOICE_RECOGNITION, InputSource.MIC), builtInSources(false))
    }
}
