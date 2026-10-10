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
class BluetoothState(context: Context, private val handler: Handler, private val onChange: () -> Unit) : BtPort {
    /**
     * F2-FR2 (device finding 2026-10-09): the profile bindings and the receiver are made on the
     * application context, not the service's. A proxy that arrives, or is closed, after the
     * recording service is destroyed (close runs on the recorder thread, after onDestroy) is
     * then not a leaked ServiceConnection of the service; ProxySlots closes it either way.
     */
    private val ctx: Context = context.applicationContext ?: context
    private val adapter: BluetoothAdapter? = ctx.getSystemService(BluetoothManager::class.java)?.adapter
    /** S15: a proxy bound after close() is closed at once, and onChange isn't called. */
    private val slots = ProxySlots<BluetoothProfile>({ profile, p -> adapter?.closeProfileProxy(profile, p) }) { handler.post(onChange) }
    private val headset get() = slots[BluetoothProfile.HEADSET] as? BluetoothHeadset
    private val a2dp get() = slots[BluetoothProfile.A2DP] as? BluetoothA2dp
    private var registered = false

    private val proxies = object : BluetoothProfile.ServiceListener {
        override fun onServiceConnected(profile: Int, proxy: BluetoothProfile) = slots.connected(profile, proxy)
        override fun onServiceDisconnected(profile: Int) = slots.disconnected(profile)
    }

    private val changes = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) = onChange()
    }

    override val ready get() = headset != null || adapter == null || !allowed() || !adapter.isEnabled

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

    override fun close() {
        if (registered) { ctx.unregisterReceiver(changes); registered = false }
        slots.close()
    }

    @SuppressLint("MissingPermission") // allowed() checks BLUETOOTH_CONNECT from API 31
    override fun snapshot(): BtSnapshot {
        val am = ctx.getSystemService(AudioManager::class.java)
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
