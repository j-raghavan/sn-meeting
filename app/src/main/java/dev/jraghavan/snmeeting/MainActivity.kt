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
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.MicChoice
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.record.BluetoothState
import dev.jraghavan.snmeeting.record.BtSnapshot
import dev.jraghavan.snmeeting.record.MicPick
import dev.jraghavan.snmeeting.record.MicSelector
import dev.jraghavan.snmeeting.record.RecState
import dev.jraghavan.snmeeting.record.RecordingService
import dev.jraghavan.snmeeting.record.usbInputs

/**
 * Home screen (spec F6) in the Inkwell style (F9): the recorder, the 5 most recent meetings,
 * and open to-dos from every meeting. Recording runs in [RecordingService], whose state arrives
 * through its hub; the lists show [MockData] until they are read from the core.
 */
class MainActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var button: TextView
    private lateinit var timer: TextView
    private lateinit var status: TextView
    private lateinit var micName: TextView
    private lateinit var micHint: TextView
    private lateinit var todoList: LinearLayout
    private lateinit var todoTitle: TextView
    private val handler = Handler(Looper.getMainLooper())
    private var rec: RecState = RecState.Idle(null, null)
    private var unobserve: (() -> Unit)? = null
    private var bt: BluetoothState? = null
    /** settings.mic, read on resume (F2-FR8); Automatic until the core answers. */
    private var micChoice: MicChoice = MicChoice.Auto

    // Ticks once a second while recording — e-ink friendly (no faster redraws).
    private val tick = object : Runnable {
        override fun run() {
            (rec as? RecState.Recording)?.let { timer.text = elapsed(it.startElapsedMs) }
            handler.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        CoreJobs.init(filesDir.absolutePath) // F1-FR5: queued first, so it runs before any other core job
        setContentView(buildView())
        render()
    }

    private fun buildView(): View {
        micName = ui.body("", 20f)
        micHint = ui.note("", 13f).apply { visibility = View.GONE }
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
                addView(micHint, ui.wrap().apply { topMargin = ui.dim(4) })
                isClickable = true
                setOnClickListener { switchMic() }
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
                setOnClickListener { startActivity(Intent(this@MainActivity, SettingsActivity::class.java)); overridePendingTransition(0, 0) }
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
        startActivity(Intent(this, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, id))
        overridePendingTransition(0, 0)
    }

    // Refresh the mic card the moment a USB mic is plugged in or pulled out.
    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) { if (!rec.busy) render() }
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) { if (!rec.busy) render() }
    }

    override fun onStart() {
        super.onStart()
        getSystemService(AudioManager::class.java).registerAudioDeviceCallback(deviceCallback, handler)
        bt = BluetoothState(this, handler) { if (!rec.busy) render() }.also { it.open() }
        unobserve = RecordingService.hub.observe { rec = it; render() }
        handler.post(tick)
    }

    override fun onStop() {
        handler.removeCallbacks(tick)
        unobserve?.invoke(); unobserve = null
        bt?.close(); bt = null
        getSystemService(AudioManager::class.java).unregisterAudioDeviceCallback(deviceCallback)
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        ensurePermissions()
        render()
        fillTodos() // a to-do may have been ticked on the meeting screen
        CoreJobs.run({ NativeBridge.nativeGetSettings() }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { if (!isDestroyed) { micChoice = it.mic; render() } }
        }
    }

    /** F2-FR8: with two mics, tapping the card switches to the other and remembers it by name. */
    private fun switchMic() {
        if (rec.busy) return // the mic can't change mid-meeting
        val next = pick().toggled() ?: return
        micChoice = next; render()
        CoreJobs.run({ NativeBridge.nativeSetSettings(SettingsPatch.mic(next)) }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { if (!isDestroyed) { micChoice = it.mic; render() } }
        }
    }

    /** The mic a new meeting would use now, or why there is none (F2-FR2). */
    private fun pick(): MicPick {
        val am = getSystemService(AudioManager::class.java)
        return MicSelector.pick(usbInputs(am), bt?.snapshot(am) ?: BtSnapshot.NONE, micChoice)
    }

    /** ＋ New starts the service; ■ Stop asks it to stop. The hub reports what happened. */
    private fun toggle() {
        if (rec.busy) {
            if (rec !is RecState.Stopping) RecordingService.stop(this)
            return
        }
        if (!ensurePermissions()) return
        val p = pick()
        if (!p.canRecord) return render()
        RecordingService.start(this, micChoice)
    }

    private fun render() {
        val r = rec
        val p = pick()
        // Idle: white pill (inkread's Open button). Recording: solid black (its Open → button).
        ui.stylePill(button, if (r.busy) "■  Stop" else "＋  New", filled = r.busy)
        if (!r.busy && !p.canRecord) button.setTextColor(ui.textMuted) // F2-AC1: disabled, reason in the mic card
        timer.visibility = if (r is RecState.Recording) View.VISIBLE else View.GONE
        if (r is RecState.Recording) timer.text = elapsed(r.startElapsedMs)
        micName.text = when (r) {
            is RecState.Starting -> r.mic
            is RecState.Recording -> r.mic
            else -> p.chosen?.label ?: p.reason
        }
        val canSwitch = !r.busy && p.toggled() != null
        micHint.visibility = if (canSwitch) View.VISIBLE else View.GONE
        micHint.text = "Tap to switch microphone"
        status.text = when (r) {
            is RecState.Starting -> "Connecting the microphone…"
            is RecState.Recording -> "Recording. The screen can sleep; it carries on."
            RecState.Stopping -> "Saving…"
            is RecState.Idle -> r.message ?: "Recordings are saved to EXPORT/sn-meeting"
        }
    }

    private fun elapsed(startElapsedMs: Long): String {
        val s = (SystemClock.elapsedRealtime() - startElapsedMs) / 1000
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
