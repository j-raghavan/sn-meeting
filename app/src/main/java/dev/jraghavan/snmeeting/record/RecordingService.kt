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
import android.util.Log
import dev.jraghavan.snmeeting.CoreJobs
import dev.jraghavan.snmeeting.MainActivity
import dev.jraghavan.snmeeting.R
import dev.jraghavan.snmeeting.StopJob
import dev.jraghavan.snmeeting.model.MicChoice

/**
 * F2-FR1: the foreground service (type microphone) that owns one recording. DEVICE-UNVERIFIED.
 * The logic lives in RecordingSession / SegmentedRecorder / MicSelector / ExitGuard; this
 * adapts them to the platform on a "recorder" HandlerThread (INV7: the session releases
 * everything on every exit).
 */
class RecordingService : Service() {

    private lateinit var thread: HandlerThread
    private lateinit var h: Handler
    private lateinit var am: AudioManager
    private var session: RecordingSession? = null

    override fun onBind(intent: Intent?): IBinder? = null

    /** Set in onDestroy: a late shutdown then only publishes (S12). */
    @Volatile private var destroyed = false
    /** The newest start: stopSelf(lastStartId) never stops a service a later start wants (N10). */
    @Volatile private var lastStartId = 0
    /** F18-FR8: the event the starting recording is for ("● Record"); null for ＋ New. */
    @Volatile private var eventKey: String? = null
    /** F15 / D14: the starting recording uses the near-live recorder (Settings → Recorder). */
    @Volatile private var nearLive = false
    /** The ongoing notification while a recording is under way; null once it ended. */
    @Volatile private var shown: Notification? = null

    override fun onCreate() {
        super.onCreate()
        life.created()
        CoreJobs.init(filesDir.absolutePath)
        am = getSystemService(AudioManager::class.java)
        thread = HandlerThread("recorder").also { t ->
            // Last resort: something escaped every guard. The looper is gone with this thread, so
            // quit it first (later core replies then run on the worker, S12), end the recording
            // in order, and defer to the default handler (a crash) only if that fails.
            val fallback = t.uncaughtExceptionHandler
            t.setUncaughtExceptionHandler { th, e ->
                t.quit()
                if (!safely.fail("uncaught", e)) fallback?.uncaughtException(th, e)
            }
            t.start()
        }
        h = Handler(thread.looper)
    }

    /** The recorder thread's one guard: a throw ends the recording as a recorder error. */
    private val safely = Safely({ what, t -> Log.w(TAG, "$what: ${t.javaClass.name}", t) }) { session?.onFault(it) }

    /** Every post to the recorder thread goes through [safely]. */
    private fun onRecorder(where: String, block: () -> Unit) = h.post { safely.run(where, block) }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStartId = startId
        when (intent?.action) {
            ACTION_START -> {
                // Foreground first: within 5 s of startForegroundService, before any work. A
                // second start while one is under way keeps the notification it already shows.
                startForeground(NOTE_ID, shown ?: notification("Starting…", null).also { shown = it },
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
                val choice = MicChoice.of(intent.getStringExtra(EXTRA_MIC))
                val event = intent.getStringExtra(EXTRA_EVENT)
                val live = intent.getBooleanExtra(EXTRA_NEAR_LIVE, false)
                onRecorder("start") { if (session == null) { eventKey = event; nearLive = live }; begin(choice) }
            }
            ACTION_STOP -> onRecorder("stop") { (session ?: RecordingSession(env).also { session = it }).stop() }
            else -> stopSelf(startId) // a restart with no intent can't resume a recording (F3-FR5)
        }
        return START_NOT_STICKY
    }

    private fun begin(choice: MicChoice) {
        if (session != null) return
        getSystemService(NotificationManager::class.java).cancel(END_NOTE_ID)
        RecordingSession(env).also { session = it }.begin(choice)
    }

    /** The platform behind RecordingSession's ports, all on the recorder thread. */
    private val env = object : RecordingSession.Env {
        override val timer: TimerPort by lazy { HandlerTimer(h, safely) }
        override fun usbInputs() = usbInputs(am)
        override fun openBluetooth(onChange: () -> Unit): BtPort =
            BluetoothState(this@RecordingService, h) { safely.run("bluetooth", onChange) }.also { it.open() }
        override fun newRecorder(listener: SegmentedRecorder.Listener) =
            SegmentedRecorder(CoreRecorderBridge(h, RetrieverDuration, safely, eventKey, live = if (nearLive) true else null),
                if (nearLive) PcmRecorderAdapter(this@RecordingService, h) else MediaRecorderAdapter(this@RecordingService, h),
                FileStorage, AndroidClock, listener,
                log = { what, t -> Log.w(TAG, "$what: ${t.javaClass.name}", t) })
        override fun newSco(listener: ScoLink.Listener) = ScoLink(AndroidScoAudio(am), HandlerTimer(h, safely), listener)
        override fun wake(): WakePort = AndroidWake(this@RecordingService)
        override fun watchDevices(): Cancel {
            am.registerAudioDeviceCallback(devices, h)
            return Cancel { am.unregisterAudioDeviceCallback(devices) }
        }
        override fun watchSco(): Cancel {
            // The SCO state broadcast is sticky (ScoLink ignores its replay).
            val filter = IntentFilter(AudioManager.ACTION_SCO_AUDIO_STATE_UPDATED).apply {
                addAction(BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED)
            }
            registerReceiver(scoReceiver, filter, null, h)
            return Cancel { unregisterReceiver(scoReceiver) }
        }
        override fun publish(s: RecState) = hub.publish(s)
        override fun showRecording(label: String, startElapsedMs: Long) =
            notify(notification(label, startElapsedMs).also { shown = it })
        override fun ended(end: RecState.Idle) = shutdown(end)
    }

    private val devices = object : AudioDeviceCallback() {
        // F2-FR3: the pinned input removed. Races the routing report on an unplug; whichever
        // reaches the recorder first ends it, the other is a no-op (SegmentedRecorder.end).
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) =
            safely.run("devices removed") { session?.onDevicesRemoved(removed.map { it.id }) }
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = safely.run("devices added") { session?.onDevicesAdded() }
    }

    /** EC10: SCO dropping or the headset disconnecting (out of range, battery). */
    private val scoReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) = safely.run("sco broadcast") {
            when (i.action) {
                AudioManager.ACTION_SCO_AUDIO_STATE_UPDATED ->
                    session?.onScoState(i.getIntExtra(AudioManager.EXTRA_SCO_AUDIO_STATE, ScoState.ERROR))
                BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED ->
                    if (i.getIntExtra(BluetoothProfile.EXTRA_STATE, -1) == BluetoothProfile.STATE_DISCONNECTED) session?.onHeadsetDisconnected()
            }
        }
    }

    /** The one exit, after the session released everything: leave the foreground, stop. */
    private fun shutdown(end: RecState.Idle) {
        session = null; shown = null
        // F15-FR9: a near-live recording's remaining chunks are sent and the minutes filled.
        val id = end.lastId
        if (id != null && nearLive) StopJob.start(id)
        if (destroyed) return // a late reply after onDestroy (S12): nothing left to stop
        stopForeground(STOP_FOREGROUND_REMOVE)
        // An ending nobody asked for (mic lost, storage full) stays in the shade (F2-FR3).
        val reason = end.message?.let(EndReason::ofMessage)
        if (reason != null && reason.leavesNotice) {
            getSystemService(NotificationManager::class.java).notify(END_NOTE_ID, ended(reason.message))
        }
        stopSelf(lastStartId)
    }

    override fun onDestroy() {
        // Stopped by the system mid-recording: finalise what we can, then release (INV7). The
        // stop queues its close and finish on CoreJobs; "running" clears after them (S12).
        destroyed = true
        val s = session
        h.post {
            safely.run("destroy") { s?.destroy() }
            life.destroyed { CoreJobs.exec(it) }
        }
        thread.quitSafely()
        if (s != null) hub.publish(RecState.Idle(null, null))
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
        const val EXTRA_EVENT = "event"
        const val EXTRA_NEAR_LIVE = "near_live"
        private const val CHANNEL = "recording"
        private const val NOTE_ID = 1
        private const val END_NOTE_ID = 2
        private const val TAG = "Meetings"

        private val life = RunningFlag()

        /** For nativeRecover(serviceAlive) and Home's ＋ New / ■ Stop. */
        val running get() = life.running

        val hub = RecordingHub { r -> Handler(Looper.getMainLooper()).post(r) }

        /** [eventKey]: "● Record" on an event links the meeting to it (F18-FR8). */
        /** [nearLive]: Settings → Recorder is Near-live (F15); else the Standard recorder. */
        fun start(c: Context, mic: MicChoice, eventKey: String? = null, nearLive: Boolean = false) =
            c.startForegroundService(Intent(c, RecordingService::class.java).setAction(ACTION_START).putExtra(EXTRA_MIC, mic.wire)
                .putExtra(EXTRA_NEAR_LIVE, nearLive)
                .apply { if (eventKey != null) putExtra(EXTRA_EVENT, eventKey) })

        fun stop(c: Context) = c.startService(Intent(c, RecordingService::class.java).setAction(ACTION_STOP))
    }
}
