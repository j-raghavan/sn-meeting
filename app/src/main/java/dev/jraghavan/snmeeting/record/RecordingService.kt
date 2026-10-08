package dev.jraghavan.snmeeting.record

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.bluetooth.BluetoothHeadset
import android.bluetooth.BluetoothProfile
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import dev.jraghavan.snmeeting.CoreJobs
import dev.jraghavan.snmeeting.MainActivity
import dev.jraghavan.snmeeting.R
import dev.jraghavan.snmeeting.model.MicChoice

/**
 * F2-FR1: the foreground service (type microphone) that owns one recording. DEVICE-UNVERIFIED.
 * The logic lives in SegmentedRecorder / MicSelector / ExitGuard; this wires them to the
 * platform on a "recorder" HandlerThread and releases everything on every exit (INV7).
 */
class RecordingService : Service() {

    private lateinit var thread: HandlerThread
    private lateinit var h: Handler
    private lateinit var am: AudioManager
    private val guard = ExitGuard()
    private var recorder: SegmentedRecorder? = null
    /** The input the recording is pinned to (F2-FR3). */
    private var pinnedId: Int? = null
    private var sco: ScoLink? = null
    private var bt: BluetoothState? = null
    /** Between ＋ New and the mic being picked (waiting for the headset proxy). */
    private var picking = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        running = true
        CoreJobs.init(filesDir.absolutePath)
        am = getSystemService(AudioManager::class.java)
        thread = HandlerThread("recorder").also { it.start() }
        h = Handler(thread.looper)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                // Foreground first: within 5 s of startForegroundService, before any work.
                startForeground(NOTE_ID, notification("Starting…", null), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
                val choice = MicChoice.of(intent.getStringExtra(EXTRA_MIC))
                h.post { begin(choice) }
            }
            ACTION_STOP -> h.post {
                val rec = recorder ?: return@post shutdown(RecState.Idle(if (picking) "Recording cancelled" else null, null))
                hub.publish(RecState.Stopping)
                rec.stop()
            }
            else -> stopSelf(startId) // a restart with no intent can't resume a recording (F3-FR5)
        }
        return START_NOT_STICKY
    }

    /**
     * Opens Bluetooth and picks the mic once the headset proxy is bound (it arrives
     * asynchronously), or after [BT_WAIT_MS]: a USB mic or a headset bound by then is offered.
     */
    private fun begin(choice: MicChoice) {
        if (recorder != null || picking) return
        picking = true
        getSystemService(NotificationManager::class.java).cancel(END_NOTE_ID)
        val state = BluetoothState(this, h) { if (bt?.ready == true) go(choice) }
        bt = state
        guard.hold("bluetooth") { state.close() }
        state.open()
        if (state.ready) go(choice) else h.postDelayed({ go(choice) }, BT_WAIT_MS)
    }

    private fun go(choice: MicChoice) {
        if (!picking) return
        picking = false
        val pick = MicSelector.pick(usbInputs(am), bt?.snapshot(am) ?: BtSnapshot.NONE, choice)
        val mic = pick.chosen ?: return shutdown(RecState.Idle(pick.reason, null))
        hub.publish(RecState.Starting(mic.label))
        val wake = WakeLease(AndroidWake(this)).also { it.begin(guard) } // F2-FR6: the screen may sleep
        val rec = SegmentedRecorder(CoreRecorderBridge(h, RetrieverDuration), MediaRecorderAdapter(this, h), FileStorage, AndroidClock,
            object : SegmentedRecorder.Listener {
                override fun onRecording(id: String, startElapsedMs: Long) {
                    hub.publish(RecState.Recording(id, mic.label, startElapsedMs))
                    notify(notification(mic.label, startElapsedMs))
                }
                override fun onSegmentStarted(index: Int) = wake.renew()
                override fun onEnded(id: String?, message: String) = shutdown(RecState.Idle(message, id))
            })
        recorder = rec
        am.registerAudioDeviceCallback(devices, h); guard.hold("deviceCallback") { am.unregisterAudioDeviceCallback(devices) }
        if (mic.kind == MicKind.USB) {
            val id = mic.deviceId ?: return shutdown(RecState.Idle(MicSelector.NONE, null))
            pinnedId = id
            rec.start(mic, id, InputSource.MIC, listOf(AudioSpec.USB))
            return
        }
        // F2-FR9: Bluetooth records only once SCO is up; nativeBeginMeeting waits for onReady.
        val link = ScoLink(AndroidScoAudio(am), HandlerTimer(h), object : ScoLink.Listener {
            override fun onReady(deviceId: Int) {
                pinnedId = deviceId
                rec.start(mic, deviceId, InputSource.VOICE_COMMUNICATION, listOf(AudioSpec.BLUETOOTH, AudioSpec.BLUETOOTH_FALLBACK))
            }
            override fun onFailed(reason: String) = shutdown(RecState.Idle(reason, null))
            override fun onLost() = rec.onMicLost()
        })
        sco = link
        // Registered before open(): the SCO state broadcast is sticky (ScoLink ignores its replay).
        val filter = IntentFilter(AudioManager.ACTION_SCO_AUDIO_STATE_UPDATED).apply {
            addAction(BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED)
        }
        registerReceiver(scoReceiver, filter, null, h); guard.hold("scoReceiver") { unregisterReceiver(scoReceiver) }
        guard.hold("sco") { link.close() } // stopBluetoothSco, then MODE_NORMAL (INV7)
        link.open()
    }

    /** F2-FR3, EC1, EC2: the pinned input going away ends the recording; nothing else is used. */
    private val devices = object : AudioDeviceCallback() {
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
            if (removed.any { it.id == pinnedId }) recorder?.onMicLost()
            sco?.onInputsChanged()
        }

        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) { sco?.onInputsChanged() }
    }

    /** EC10: SCO dropping or the headset disconnecting (out of range, battery). */
    private val scoReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            when (i.action) {
                AudioManager.ACTION_SCO_AUDIO_STATE_UPDATED ->
                    sco?.onScoState(i.getIntExtra(AudioManager.EXTRA_SCO_AUDIO_STATE, ScoState.ERROR))
                BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED ->
                    if (i.getIntExtra(BluetoothProfile.EXTRA_STATE, -1) == BluetoothProfile.STATE_DISCONNECTED) sco?.onHeadsetDisconnected()
            }
        }
    }

    /** The one exit: release everything, publish, leave the foreground, stop. */
    private fun shutdown(end: RecState.Idle) {
        guard.releaseAll()
        recorder = null; pinnedId = null; sco = null; bt = null; picking = false
        hub.publish(end)
        stopForeground(STOP_FOREGROUND_REMOVE)
        // An ending nobody asked for (mic lost, storage full) stays in the shade (F2-FR3).
        val reason = end.message?.let(EndReason::ofMessage)
        if (reason != null && reason.leavesNotice) {
            getSystemService(NotificationManager::class.java).notify(END_NOTE_ID, ended(reason.message))
        }
        stopSelf()
    }

    override fun onDestroy() {
        // Stopped by the system mid-recording: finalise what we can, then release (INV7).
        val rec = recorder
        h.post { rec?.stop(); guard.releaseAll() }
        thread.quitSafely()
        if (rec != null) hub.publish(RecState.Idle(null, null))
        running = false
        super.onDestroy()
    }

    private fun notification(text: String, startElapsedMs: Long?): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "Recording", NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(this, 1, Intent(this, RecordingService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_meetings)
            .setContentTitle("Meetings")
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .apply {
                if (startElapsedMs != null) {
                    // The system ticks the elapsed time; we post only on state changes.
                    setUsesChronometer(true)
                    setWhen(System.currentTimeMillis() - (SystemClock.elapsedRealtime() - startElapsedMs))
                }
            }
            .build()
    }

    private fun ended(text: String): Notification =
        Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_meetings)
            .setContentTitle("Meetings")
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE))
            .build()

    private fun notify(n: Notification) = getSystemService(NotificationManager::class.java).notify(NOTE_ID, n)

    companion object {
        const val ACTION_START = "dev.jraghavan.snmeeting.record.START"
        const val ACTION_STOP = "dev.jraghavan.snmeeting.record.STOP"
        const val EXTRA_MIC = "mic"
        private const val CHANNEL = "recording"
        private const val NOTE_ID = 1
        private const val END_NOTE_ID = 2
        private const val BT_WAIT_MS = 1_500L

        /** For nativeRecover(serviceAlive) and Home's ＋ New / ■ Stop. */
        @Volatile var running = false
            private set

        val hub = RecordingHub { r -> Handler(Looper.getMainLooper()).post(r) }

        fun start(c: Context, mic: MicChoice) =
            c.startForegroundService(Intent(c, RecordingService::class.java).setAction(ACTION_START).putExtra(EXTRA_MIC, mic.wire))

        fun stop(c: Context) = c.startService(Intent(c, RecordingService::class.java).setAction(ACTION_STOP))
    }
}
