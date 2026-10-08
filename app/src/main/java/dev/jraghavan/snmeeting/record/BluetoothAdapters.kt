package dev.jraghavan.snmeeting.record

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothA2dp
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothHeadset
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler

// DEVICE-UNVERIFIED: thin Bluetooth adapters (F2-FR9, F2-FR10, EC11, EC12). The decisions they
// feed are in MicSelector and ScoLink, which are host-tested.

class AndroidScoAudio(private val am: AudioManager) : ScoAudio {
    override fun setCommunicationMode(on: Boolean) { am.mode = if (on) AudioManager.MODE_IN_COMMUNICATION else AudioManager.MODE_NORMAL }
    @Suppress("DEPRECATION") override fun startSco() = am.startBluetoothSco()
    @Suppress("DEPRECATION") override fun stopSco() = am.stopBluetoothSco()
    override fun scoInputId(): Int? =
        am.getDevices(AudioManager.GET_DEVICES_INPUTS).firstOrNull { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }?.id
}

/**
 * Bluetooth as names (F2-FR2, EC11, EC12): HEADSET and A2DP profile proxies once bound (they
 * arrive asynchronously; until then it reports what it can), plus the adapter and headset
 * broadcasts, each of which calls [onChange] on [handler]'s thread. API 30: BLUETOOTH is a normal
 * permission; from API 31 BLUETOOTH_CONNECT is checked and, if denied, Bluetooth reads as absent.
 */
class BluetoothState(private val ctx: Context, private val handler: Handler, private val onChange: () -> Unit) {
    private val adapter: BluetoothAdapter? = ctx.getSystemService(BluetoothManager::class.java)?.adapter
    private var headset: BluetoothHeadset? = null
    private var a2dp: BluetoothA2dp? = null
    private var registered = false

    private val proxies = object : BluetoothProfile.ServiceListener {
        override fun onServiceConnected(profile: Int, proxy: BluetoothProfile) {
            if (profile == BluetoothProfile.HEADSET) headset = proxy as BluetoothHeadset
            if (profile == BluetoothProfile.A2DP) a2dp = proxy as BluetoothA2dp
            handler.post(onChange)
        }

        override fun onServiceDisconnected(profile: Int) {
            if (profile == BluetoothProfile.HEADSET) headset = null
            if (profile == BluetoothProfile.A2DP) a2dp = null
            handler.post(onChange)
        }
    }

    private val changes = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) = onChange()
    }

    /** The headset proxy is bound (or never will be), so [snapshot] can list HFP headsets. */
    val ready get() = headset != null || adapter == null || !allowed() || !adapter.isEnabled

        private fun allowed(): Boolean = Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
        ctx.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED

    fun open() {
        if (adapter == null || !allowed()) return
        adapter.getProfileProxy(ctx, proxies, BluetoothProfile.HEADSET)
        adapter.getProfileProxy(ctx, proxies, BluetoothProfile.A2DP)
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED).apply {
            addAction(BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED)
            addAction(BluetoothA2dp.ACTION_CONNECTION_STATE_CHANGED)
        }
        ctx.registerReceiver(changes, filter, null, handler)
        registered = true
    }

    fun close() {
        if (registered) { ctx.unregisterReceiver(changes); registered = false }
        headset?.let { adapter?.closeProfileProxy(BluetoothProfile.HEADSET, it) }
        a2dp?.let { adapter?.closeProfileProxy(BluetoothProfile.A2DP, it) }
        headset = null; a2dp = null
    }

    @SuppressLint("MissingPermission") // allowed() checks BLUETOOTH_CONNECT from API 31
    fun snapshot(am: AudioManager): BtSnapshot {
        val a = adapter ?: return BtSnapshot.NONE
        if (!allowed()) return BtSnapshot.NONE
        val on = a.isEnabled
        fun BluetoothDevice.label() = name?.takeIf { it.isNotBlank() } ?: address
        val hfp = headset?.connectedDevices.orEmpty()
        val music = a2dp?.connectedDevices.orEmpty().filter { d -> hfp.none { it.address == d.address } }
        val bonded = if (on) a.bondedDevices.orEmpty().filter { it.bluetoothClass?.majorDeviceClass == BluetoothClass.Device.Major.AUDIO_VIDEO } else emptyList()
        return BtSnapshot(on, hfp.map { it.label() }, music.map { it.label() }, bonded.map { it.label() }, am.isBluetoothScoAvailableOffCall)
    }
}
