package dev.jraghavan.snmeeting

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.TextView
import java.io.File

/**
 * Home screen (spec F6) in the Inkwell style (F9): the recorder, the 5 most recent meetings,
 * and open to-dos from every meeting. The recorder is real; the lists show [MockData] until the
 * Rust core's library (F3) exists.
 */
class MainActivity : Activity() {

    private val outputDir = File(Environment.getExternalStorageDirectory(), "EXPORT/sn-meeting")
    private lateinit var ui: Inkwell
    private lateinit var recorder: MeetingRecorder
    private lateinit var button: TextView
    private lateinit var timer: TextView
    private lateinit var status: TextView
    private lateinit var micName: TextView
    private lateinit var todoList: LinearLayout
    private lateinit var todoTitle: TextView
    private val handler = Handler(Looper.getMainLooper())
    private var startedAt = 0L

    // Ticks once a second while recording — e-ink friendly (no faster redraws).
    private val tick = object : Runnable {
        override fun run() {
            timer.text = elapsed()
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        initCore()
        recorder = MeetingRecorder(this, outputDir)
        setContentView(buildView())
        render()
    }

    /** Starts the Rust core (F1-FR5). Queued first, so it runs before any other core job. */
    private fun initCore() {
        val dir = filesDir.absolutePath
        CoreJobs.run({ NativeBridge.nativeInit(dir) }) { Log.i("Meetings", "nativeInit: $it") }
    }

    private fun buildView(): View {
        micName = ui.body("", 20f)
        timer = ui.body("", 44f).apply { gravity = Gravity.CENTER; visibility = View.GONE }
        button = ui.pill("＋  New", size = 20f) { toggle() }
        status = ui.note("", 15f).apply { gravity = Gravity.CENTER }
        todoList = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        todoTitle = ui.eyebrowItalic("")

        // Not a scrolling page: the header block is fixed and the two lists share the remaining
        // height equally, each scrolling inside its own card (F6-FR1, owner decision 2026-10-07).
        val column = ui.column().apply {
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
            addView(ui.brandBlock("·  Meeting Recorder · v${ui.versionName()}  ·"))
            addView(ui.spacer(ui.dim(32)))
            addView(ui.card().apply {
                addView(ui.eyebrow("Microphone"))
                addView(micName, ui.wrap().apply { topMargin = ui.dim(6) })
            })
            addView(ui.spacer(ui.dim(28)))
            addView(timer)
            addView(button, ui.wrap().apply { topMargin = ui.dim(8) })
            addView(ui.spacer(ui.dim(16)))
            addView(status)
            addView(ui.spacer(ui.dim(36)))
            val meetings = MockData.meetings.take(5)
            addView(sideBySide(
                section(ui.eyebrowItalic("Recent meetings  ·  ${meetings.size}"), meetingList(meetings)),
                section(todoTitle, todoList),
            ), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.closingMark())
        }
        fillTodos()
        // A subtle top-corner gear → Settings, as on inkread's home screen (F10-FR1).
        return FrameLayout(this).apply {
            setBackgroundColor(android.graphics.Color.WHITE)
            addView(column, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            addView(ImageView(this@MainActivity).apply {
                setImageResource(R.drawable.ic_settings)
                val p = ui.dp(8); setPadding(p, p, p, p)
                isClickable = true
                setOnClickListener {
                    if (!recorder.isRecording) { startActivity(Intent(this@MainActivity, SettingsActivity::class.java)); overridePendingTransition(0, 0) }
                }
            }, FrameLayout.LayoutParams(ui.dp(40), ui.dp(40), Gravity.TOP or Gravity.END).apply { topMargin = ui.dp(10); marginEnd = ui.dp(10) })
        }
    }

    /** Two equal columns, equal heights, with a gutter (F6-FR1: meetings left, to-dos right). */
    private fun sideBySide(left: View, right: View): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        addView(left, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f).apply { marginEnd = ui.dim(12) })
        addView(right, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f).apply { marginStart = ui.dim(12) })
    }

    /** Heading over a card that fills the column's height and scrolls inside. */
    private fun section(title: TextView, rows: LinearLayout): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        addView(title, ui.fullWidth())
        addView(ui.spacer(ui.dim(14)))
        addView(ui.scrollCard(rows), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    /** The most recent meetings as keyline rows (F6-FR1). */
    private fun meetingList(meetings: List<MockMeeting>): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        meetings.forEachIndexed { i, m ->
            if (i > 0) addView(ui.hairline())
            addView(LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                isClickable = true
                setOnClickListener { openMeeting(m.id) }
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    // Title first (serif, bold); untitled meetings read muted until extraction names them.
                    addView(ui.body(m.displayTitle, 18f, bold = m.title != null).apply {
                        if (m.title == null) { setTextColor(ui.textMuted); typeface = ui.serifItalic }
                    })
                    addView(ui.eyebrow("${m.day}  ·  ${m.time}"), ui.wrap().apply { topMargin = ui.dim(4) })
                    val todos = when {
                        m.status != MockStatus.DONE -> ""
                        m.openTodos == 0 -> "  ·  no open to-dos"
                        m.openTodos == 1 -> "  ·  1 open to-do"
                        else -> "  ·  ${m.openTodos} open to-dos"
                    }
                    addView(ui.note(m.duration + todos), ui.wrap().apply { topMargin = ui.dim(3) })
                    // Half-width column: the status chip sits under the details, not beside them.
                    addView(ui.chip(m.status.label, filled = m.status.attention), ui.wrap().apply { topMargin = ui.dim(8) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.body("→", 18f).apply { setPadding(ui.dim(16), 0, 0, 0) })
            })
        }
    }

    /** Open to-dos from every meeting, not just the 5 listed (F6-FR1); ticking one hides it. */
    private fun fillTodos() {
        todoList.removeAllViews()
        val open = MockData.meetings.flatMap { m -> m.todos.filter { !it.done }.map { m to it } }
        todoTitle.text = "Open to-dos  ·  ${open.size}"
        if (open.isEmpty()) {
            todoList.addView(ui.note("Nothing open. To-dos appear here after a meeting is transcribed.").apply {
                setPadding(0, ui.dim(14), 0, ui.dim(14))
            })
            return
        }
        open.forEachIndexed { i, (m, t) ->
            if (i > 0) todoList.addView(ui.hairline())
            todoList.addView(LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                addView(ui.checkbox(t.done).apply {
                    isClickable = true
                    setOnClickListener { t.done = true; fillTodos() }
                })
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(ui.dim(16), 0, 0, 0)
                    addView(ui.body(t.text, 17f))
                    addView(ui.note("${m.day}  ·  ${t.meta}", 13f), ui.wrap().apply { topMargin = ui.dim(3) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
        }
    }

    private fun openMeeting(id: String) {
        if (recorder.isRecording) return
        startActivity(Intent(this, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, id))
        overridePendingTransition(0, 0)
    }

    // Refresh the mic card the moment a USB mic is plugged in or pulled out.
    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) { if (!recorder.isRecording) render() }
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) { if (!recorder.isRecording) render() }
    }

    override fun onStart() {
        super.onStart()
        getSystemService(AudioManager::class.java).registerAudioDeviceCallback(deviceCallback, handler)
    }

    override fun onStop() {
        getSystemService(AudioManager::class.java).unregisterAudioDeviceCallback(deviceCallback)
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        ensurePermissions()
        if (!recorder.isRecording) render()
        fillTodos() // a to-do may have been ticked on the meeting screen
    }

    override fun onDestroy() {
        handler.removeCallbacks(tick)
        recorder.stop()
        super.onDestroy()
    }

    private fun toggle() {
        if (recorder.isRecording) {
            handler.removeCallbacks(tick)
            val file = recorder.stop()
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            render(if (file != null) "Saved " + file.name + "  ·  " + elapsed() else "Too short — nothing saved")
            return
        }
        if (!ensurePermissions()) return
        if (recorder.usbInput() == null) {
            render("Plug in the USB microphone first")
            return
        }
        try {
            recorder.start()
        } catch (e: Exception) {
            render("Could not start: " + e.message)
            return
        }
        startedAt = SystemClock.elapsedRealtime()
        // Keep the device awake for the length of the meeting.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        render("Recording to " + recorder.currentFile?.name)
        handler.post(tick)
    }

    private fun render(message: String? = null) {
        val recording = recorder.isRecording
        // Idle: white pill (inkread's Open button). Recording: solid black (its Open → button).
        ui.stylePill(button, if (recording) "■  Stop" else "＋  New", filled = recording)
        timer.visibility = if (recording) View.VISIBLE else View.GONE
        if (recording) timer.text = elapsed()
        micName.text = recorder.usbInput()?.productName?.toString()?.let { "USB — $it" } ?: "None — plug in a USB microphone"
        if (message != null) status.text = message
        else if (!recording) status.text = "Recordings are saved to EXPORT/sn-meeting"
    }

    private fun elapsed(): String {
        val s = (SystemClock.elapsedRealtime() - startedAt) / 1000
        return "%d:%02d:%02d".format(s / 3600, (s / 60) % 60, s % 60)
    }

    /** Returns true when both mic and all-files access are granted; otherwise asks for them. */
    private fun ensurePermissions(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
            return false
        }
        if (!Environment.isExternalStorageManager()) {
            startActivity(
                Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
            )
            return false
        }
        return true
    }
}
