package dev.jraghavan.snmeeting.record

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
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
                val rec = recorder ?: return@post shutdown(RecState.Idle(null, null))
                hub.publish(RecState.Stopping)
                rec.stop()
            }
            else -> stopSelf(startId) // a restart with no intent can't resume a recording (F3-FR5)
        }
        return START_NOT_STICKY
    }

    private fun begin(choice: MicChoice) {
        if (recorder != null) return
        val pick = MicSelector.pick(usbInputs(am), BtSnapshot.NONE, choice)
        val mic = pick.chosen ?: return shutdown(RecState.Idle(pick.reason, null))
        hub.publish(RecState.Starting(mic.label))
        val rec = SegmentedRecorder(CoreRecorderBridge(h, RetrieverDuration), MediaRecorderAdapter(this), FileStorage, AndroidClock,
            object : SegmentedRecorder.Listener {
                override fun onRecording(id: String, startElapsedMs: Long) {
                    hub.publish(RecState.Recording(id, mic.label, startElapsedMs))
                    notify(notification(mic.label, startElapsedMs))
                }
                override fun onSegmentStarted(index: Int) = Unit
                override fun onEnded(id: String?, message: String) = shutdown(RecState.Idle(message, id))
            })
        recorder = rec
        rec.start(mic, mic.deviceId ?: return shutdown(RecState.Idle(MicSelector.NONE, null)), InputSource.MIC, listOf(AudioSpec.USB))
    }

    /** The one exit: release everything, publish, leave the foreground, stop. */
    private fun shutdown(end: RecState.Idle) {
        guard.releaseAll()
        recorder = null
        hub.publish(end)
        stopForeground(STOP_FOREGROUND_REMOVE)
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

    private fun notify(n: Notification) = getSystemService(NotificationManager::class.java).notify(NOTE_ID, n)

    companion object {
        const val ACTION_START = "dev.jraghavan.snmeeting.record.START"
        const val ACTION_STOP = "dev.jraghavan.snmeeting.record.STOP"
        const val EXTRA_MIC = "mic"
        private const val CHANNEL = "recording"
        private const val NOTE_ID = 1

        /** For nativeRecover(serviceAlive) and Home's ＋ New / ■ Stop. */
        @Volatile var running = false
            private set

        val hub = RecordingHub { r -> Handler(Looper.getMainLooper()).post(r) }

        fun start(c: Context, mic: MicChoice) =
            c.startForegroundService(Intent(c, RecordingService::class.java).setAction(ACTION_START).putExtra(EXTRA_MIC, mic.wire))

        fun stop(c: Context) = c.startService(Intent(c, RecordingService::class.java).setAction(ACTION_STOP))
    }
}
