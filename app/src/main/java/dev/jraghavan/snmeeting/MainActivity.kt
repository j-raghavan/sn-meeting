package dev.jraghavan.snmeeting

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.Agenda
import dev.jraghavan.snmeeting.model.AgendaRow
import dev.jraghavan.snmeeting.model.CalendarFormat
import dev.jraghavan.snmeeting.model.CalendarPrefs
import dev.jraghavan.snmeeting.model.CalendarRefresh
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.HomeRows
import dev.jraghavan.snmeeting.model.MeetingSummary
import dev.jraghavan.snmeeting.model.MicChoice
import dev.jraghavan.snmeeting.model.Permissions
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.Spend
import dev.jraghavan.snmeeting.model.SpendReport
import dev.jraghavan.snmeeting.model.TodoRef
import dev.jraghavan.snmeeting.record.BluetoothState
import dev.jraghavan.snmeeting.record.BtSnapshot
import dev.jraghavan.snmeeting.record.MicPick
import dev.jraghavan.snmeeting.record.MicSelector
import dev.jraghavan.snmeeting.record.RecState
import dev.jraghavan.snmeeting.record.Recoverer
import dev.jraghavan.snmeeting.record.RecordingService
import dev.jraghavan.snmeeting.record.RetrieverDuration
import dev.jraghavan.snmeeting.record.usbInputs
import dev.jraghavan.snmeeting.update.UpdateGate
import java.time.LocalDate
import java.time.ZoneId

/**
 * Home, layout A "Agenda and To-dos" (spec F18-FR7, owner mock-up 2026-10-09), in the Inkwell
 * style (F9): the date, the wordmark with this month's spend, ⌕ and ⚙; the week strip; the
 * chosen day's events and recordings beside the open to-dos; the mic line; and the bottom row
 * "CALENDAR · UPDATED hh:mm · ＋ New". Events come from the core's calendar cache, so Home
 * works offline (F18-AC9); with no link set it is built from recordings alone (F18-FR14).
 *
 * Redraws (NFR2, e-ink): on resume, when a refresh or a job ends, on recording transitions, and
 * at the minute the current event changes; never faster. The recording timer is the one view
 * that ticks, once a second.
 */
class MainActivity : Activity(), HomeLists.Actions {

    private lateinit var ui: Inkwell
    private lateinit var lists: HomeLists
    private lateinit var dateText: TextView
    /** F16-FR5: "October · $3.20 spent · 6 meetings", under the wordmark. */
    private lateinit var spendLine: TextView
    private lateinit var micLine: TextView
    private lateinit var calendarCell: TextView
    private lateinit var updatedCell: TextView
    private lateinit var button: TextView
    private val handler = Handler(Looper.getMainLooper())
    private var rec: RecState = RecState.Idle(null, null)
    private var unobserve: (() -> Unit)? = null
    private var unlisten: (() -> Unit)? = null
    private var bt: BluetoothState? = null
    private var micChoice: MicChoice = MicChoice.Auto
    /** F15: Settings → Recorder; Standard until the settings are read. */
    private var nearLive = false
    private var saveFolder = "EXPORT/sn-meeting"
    private var prefs = CalendarPrefs()

    private val zone: ZoneId get() = ZoneId.systemDefault()
    private var today: LocalDate = LocalDate.now()
    private var selected: LocalDate = LocalDate.now()
    private var agenda: Agenda? = null
    /** The last agenda and to-do replies drawn: the same reply again draws nothing. */
    private var drawnAgenda: String? = null
    private var drawnTodos: String? = null
    /** "● Record" in an event's box or sheet: the event the next recording is for (F18-FR8). */
    private var pendingEvent: String? = null
    /** The template chosen in the event sheet, applied once its recording has an id. */
    private var pendingTemplate: String? = null

    /** F6-FR4: lists re-read on resume, on recording transitions and when a job ends; ≤ 1/s. */
    private val gate = RefreshGate(SystemClock::elapsedRealtime, { ms, r ->
        val task = Runnable(r)
        handler.postDelayed(task, ms)
        ({ handler.removeCallbacks(task) })
    }) { reload() }

    // The recording timer, once a second, only while recording.
    private val tick = object : Runnable {
        override fun run() {
            val r = rec as? RecState.Recording ?: return
            val t = elapsed(r.startElapsedMs)
            lists.timer?.text = "● $t"
            micLine.text = "Recording · $t"
            handler.postDelayed(this, 1000)
        }
    }

    /** The minute the current event (or the day) changes: redraw then, and only then. */
    private val boundary = Runnable { today = LocalDate.now(); gate.request() }

    /** F18-FR4: the automatic refresh while Home is open. */
    private val autoRefresh = Runnable { refreshCalendar(force = false) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        CoreJobs.init(filesDir.absolutePath) // F1-FR5: queued first, so it runs before any other core job
        SearchJobs.init(filesDir.absolutePath)
        CalendarJobs.init(filesDir.absolutePath)
        setContentView(buildView())
        render()
        handleIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /** Search's event sheet asks Home to record its event (F19-FR3 → F18-FR8). */
    private fun handleIntent(i: Intent?) {
        val key = i?.getStringExtra(EXTRA_RECORD_EVENT) ?: return
        i.removeExtra(EXTRA_RECORD_EVENT)
        recordFor(key, i.getStringExtra(EXTRA_TEMPLATE))
    }

    private fun icon(res: Int, description: String, onClick: () -> Unit) = ImageView(this).apply {
        setImageResource(res)
        val p = ui.dp(8); setPadding(p, p, p, p)
        contentDescription = description
        isClickable = true
        setOnClickListener { onClick() }
    }

    private fun buildView(): View {
        lists = HomeLists(this, ui, this)
        dateText = ui.body("", 18f)
        spendLine = ui.eyebrowItalic("").apply { visibility = View.GONE }
        micLine = ui.note("", 14f).apply {
            gravity = Gravity.CENTER; isClickable = true; setOnClickListener { switchMic() }
        }
        calendarCell = ui.eyebrow("Calendar").apply {
            isClickable = true; setOnClickListener { openCalendarSettings() }
            setPadding(0, ui.dim(10), ui.dim(8), ui.dim(10))
        }
        updatedCell = ui.eyebrow("", ui.inkSoft).apply {
            gravity = Gravity.CENTER; isClickable = true; setOnClickListener { refreshCalendar(force = true) }
            setPadding(ui.dim(8), ui.dim(10), ui.dim(8), ui.dim(10))
        }
        button = ui.pill("＋  New", size = 18f) { toggle() }

        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(dateText, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.lockup())
            addView(LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.END or Gravity.CENTER_VERTICAL
                addView(icon(R.drawable.ic_search, "Search") {
                    startActivity(Intent(this@MainActivity, SearchActivity::class.java)); overridePendingTransition(0, 0)
                }, LinearLayout.LayoutParams(ui.dp(40), ui.dp(40)))
                addView(icon(R.drawable.ic_settings, "Settings") {
                    startActivity(Intent(this@MainActivity, SettingsActivity::class.java)); overridePendingTransition(0, 0)
                }, LinearLayout.LayoutParams(ui.dp(40), ui.dp(40)).apply { marginStart = ui.dp(8) })
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
        val bottom = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(calendarCell, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(updatedCell, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(LinearLayout(this@MainActivity).apply {
                gravity = Gravity.END or Gravity.CENTER_VERTICAL
                addView(button)
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
        // Not a scrolling page: header, week and bottom row are fixed; the two columns share the
        // rest, each scrolling inside its own card (as F6-FR1's owner decision).
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.side, ui.dim(24), ui.side, ui.dim(20))
            addView(header, ui.fullWidth())
            addView(spendLine, ui.fullWidth().apply { topMargin = ui.dim(6) })
            addView(ui.spacer(ui.dim(18)))
            addView(lists.week, ui.fullWidth())
            addView(ui.spacer(ui.dim(18)))
            addView(ui.hairline())
            addView(ui.spacer(ui.dim(16)))
            addView(lists.columns, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(ui.spacer(ui.dim(14)))
            addView(micLine, ui.fullWidth())
            addView(ui.spacer(ui.dim(10)))
            addView(ui.hairline())
            addView(bottom, ui.fullWidth().apply { topMargin = ui.dim(8) })
        }
        return FrameLayout(this).apply {
            setBackgroundColor(android.graphics.Color.WHITE)
            addView(column, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
    }

    override fun onMeeting(id: String) = openMeeting(id, null)

    private fun openMeeting(id: String, focus: String?) {
        val i = Intent(this, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, id)
        focus?.let { i.putExtra(MeetingActivity.EXTRA_FOCUS, it) }
        startActivity(i)
        overridePendingTransition(0, 0)
    }

    // Refresh the mic line the moment a USB mic is plugged in or pulled out.
    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) { if (!rec.busy) render() }
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) { if (!rec.busy) render() }
    }

    override fun onStart() {
        super.onStart()
        getSystemService(AudioManager::class.java).registerAudioDeviceCallback(deviceCallback, handler)
        bt = BluetoothState(this, handler) { if (!rec.busy) render() }.also { it.open() }
        unobserve = RecordingService.hub.observe { next ->
            val changed = next.javaClass != rec.javaClass
            rec = next; render()
            if (next is RecState.Recording) applyPendingTemplate(next.id)
            if (changed) {
                gate.request() // a meeting began or ended: its row and marks change
                handler.removeCallbacks(tick)
                if (next is RecState.Recording) handler.post(tick)
            }
        }
        unlisten = LongJobs.lane.onFinished { gate.request(); SearchJobs.refresh() } // F19-FR5
        if (rec is RecState.Recording) { handler.removeCallbacks(tick); handler.post(tick) }
    }

    override fun onStop() {
        handler.removeCallbacks(tick)
        handler.removeCallbacks(boundary)
        handler.removeCallbacks(autoRefresh) // F18-FR4: only while Home is in the foreground
        unobserve?.invoke(); unobserve = null
        unlisten?.invoke(); unlisten = null
        gate.stop()
        bt?.close(); bt = null
        getSystemService(AudioManager::class.java).unregisterAudioDeviceCallback(deviceCallback)
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        val now = LocalDate.now()
        if (now != today) { today = now; selected = now } // a new day starts on today
        render()
        if (access.onResume(Environment.isExternalStorageManager()) == Permissions.OnResume.STORAGE_CARD) {
            gate.stop()
            drawnAgenda = null; drawnTodos = null
            lists.showStorageNeeded(Permissions.STORAGE_TEXT) { grantStorage() }
        } else {
            recover()
            gate.request() // queued after recovery, so recovered meetings show their new status
            SearchJobs.refresh() // F19-FR5: re-index what changed, off the core queue
        }
        CoreJobs.run({ NativeBridge.nativeGetSettings() }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { if (!isDestroyed) { micChoice = it.mic; saveFolder = it.saveFolder; nearLive = it.nearLive; render() } }
            prefs = CoreJson.obj(json) { o -> CalendarPrefs.from(o.optJSONObject("settings")?.optJSONObject("calendar")) }.getOrNull() ?: prefs
            refreshCalendar(force = false) // the links file may have changed on the desktop (F18-FR1)
        }
        // F14-FR8: the throttled release check (as inkread's HomeActivity); quiet while recording.
        UpdateGate.maybeCheckOnLaunch(this)
    }

    /**
     * F3-FR5 on launch and resume: meetings a crash left recording or transcribing are moved on,
     * and segments that were never closed are measured and closed. A live service is left alone.
     */
    private fun recover() {
        val alive = RecordingService.running
        CoreJobs.exec {
            Recoverer(NativeBridge::nativeRecover, RetrieverDuration) { id, i, ms -> NativeBridge.nativeCloseSegment(id, i, ms) }
                .run(alive)
        }
    }

    /**
     * F18-FR4: fetch the due links (all of them when [force]: a tap on "UPDATED"). Automatic
     * refreshes wait while a provider job may be uploading; the core
     * waits out a recording's first 10 s. The next one is scheduled at the interval.
     */
    private fun refreshCalendar(force: Boolean) {
        handler.removeCallbacks(autoRefresh)
        if (!force && !CalendarRefresh.autoAllowed(LongJobs.lane.hasMeetingJob())) return scheduleRefresh()
        if (force) updatedCell.text = "UPDATING…"
        CalendarJobs.refresh(force) { _ ->
            if (isDestroyed) return@refresh
            if (force) drawnAgenda = null // the UPDATED time is redrawn even if no event changed
            gate.request()
            scheduleRefresh()
        }
    }

    private fun scheduleRefresh() {
        handler.removeCallbacks(autoRefresh)
        CalendarRefresh.intervalMs(prefs.refreshMin)?.let { handler.postDelayed(autoRefresh, it) }
    }

    /** The shown week's agenda, this month's spend and every open to-do, from the core. */
    private fun reload() {
        if (!Environment.isExternalStorageManager()) return // the all-files card stays (S1)
        val week = CalendarFormat.week(selected)
        CoreJobs.run({ NativeBridge.nativeAgenda(week.first().toString(), week.last().toString()) }) { json ->
            if (isDestroyed || json == drawnAgenda) return@run
            drawnAgenda = json
            when (val r = CoreJson.obj(json, Agenda::from)) {
                is CoreResult.Ok -> { agenda = r.value; drawAgenda() }
                is CoreResult.Err -> { agenda = null; lists.showDay(null, selected, today, false, rec is RecState.Recording, r.message) }
            }
        }
        // F16-FR5: this month's spend, redrawn only when its text changes.
        CoreJobs.run({ val m = Spend.key(java.time.YearMonth.now()); NativeBridge.nativeSpend(m, m) }) { json ->
            if (isDestroyed) return@run
            val line = CoreJson.obj(json, SpendReport::from).getOrNull()?.months?.firstOrNull()?.label
            if (line.isNullOrEmpty()) { spendLine.visibility = View.GONE; return@run }
            if (spendLine.text.toString() != line) spendLine.text = line
            spendLine.visibility = View.VISIBLE
        }
        CoreJobs.run({ NativeBridge.nativeOpenTodos() }) { json ->
            if (isDestroyed || json == drawnTodos) return@run
            drawnTodos = json
            when (val r = CoreJson.list(json, TodoRef::from)) {
                is CoreResult.Ok -> lists.showTodos(r.value)
                is CoreResult.Err -> lists.showTodos(emptyList(), r.message)
            }
        }
    }

    /** One redraw of the week strip, the day and the bottom row from [agenda]. */
    private fun drawAgenda() {
        val a = agenda ?: return
        val week = CalendarFormat.week(selected)
        lists.showWeek(week, today, selected, a.days.filter { it.dot }.map { it.day }.toSet())
        lists.showDay(a.day(selected), selected, today, a.hasLinks, rec is RecState.Recording)
        if (rec is RecState.Recording) { handler.removeCallbacks(tick); handler.post(tick) }
        // F18-FR14: no link set: the bottom row offers the how-to instead of a refresh time.
        calendarCell.text = if (a.hasLinks) "CALENDAR" else "CALENDAR · ADD A CALENDAR LINK"
        updatedCell.text = if (a.hasLinks) CalendarFormat.updated(a.updatedAtMs, a.failed, zone).uppercase() else ""
        updatedCell.isClickable = a.hasLinks
        scheduleBoundary(a)
    }

    /** Wake at the next start or end today (the boxed row may change), or at midnight. */
    private fun scheduleBoundary(a: Agenda) {
        handler.removeCallbacks(boundary)
        val now = System.currentTimeMillis()
        val midnight = today.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
        val next = CalendarFormat.nextChange(a.day(today)?.rows.orEmpty(), now)?.coerceAtMost(midnight) ?: midnight
        handler.postDelayed(boundary, (next - now).coerceAtLeast(1000) + 1000)
    }

    override fun onDay(d: LocalDate) {
        val sameWeek = CalendarFormat.week(d).first() == CalendarFormat.week(selected).first()
        selected = d
        if (sameWeek) drawAgenda() else { drawnAgenda = null; gate.request() }
    }

    /** F18-FR8: the event's sheet. */
    override fun onEvent(r: AgendaRow) {
        val key = r.eventKey ?: return
        EventSheetDialog.open(this, ui, key, micLine.text.toString()) { k, t -> recordFor(k, t) }
    }

    /** F18-FR10: an unscheduled recording asks which event it was. */
    override fun onUnscheduled(r: AgendaRow) {
        val id = r.meetingId ?: return
        EventLinkPicker.show(this, id, r.startMs, r.endMs, null) { drawnAgenda = null; gate.request() }
    }

    /** "● Record" in the current event's box. */
    override fun onRecordEvent(r: AgendaRow) { r.eventKey?.let { recordFor(it, null) } }

    /** Start a meeting for event [key] (F18-FR8), through ＋ New's permission checks. */
    private fun recordFor(key: String, templateId: String?) {
        if (rec.busy) return InkDialog.error(this, "Already recording", "Stop the current meeting first.")
        pendingEvent = key
        pendingTemplate = templateId
        toggle()
    }

    /** The template picked in the event sheet goes on the meeting once it has an id. */
    private fun applyPendingTemplate(id: String) {
        val t = pendingTemplate ?: return
        pendingTemplate = null
        CoreJobs.run({ NativeBridge.nativeSetMeetingTemplate(id, t) }) { }
    }

    override fun onTick(t: TodoRef) {
        CoreJobs.run({ NativeBridge.nativeSetTodoDone(t.meetingId, t.todoId, true) }) { json ->
            if (isDestroyed) return@run
            val r = CoreJson.ok(json)
            if (r is CoreResult.Err) InkDialog.error(this, "Couldn't tick the to-do", r.message)
            drawnTodos = null; drawnAgenda = null // its marks and "From last time" change too
            gate.request()
        }
    }

    /** A to-do's source opens its meeting on the minutes (the moment it was said is M12's). */
    override fun onTodoSource(t: TodoRef) = openMeeting(t.meetingId, "todos")

    /** OQ22: "Recent ›" lists the five newest meetings (F6-AC1 applies to this list). */
    override fun onRecent() {
        CoreJobs.run({ NativeBridge.nativeListRecent(RECENT) }) { json ->
            if (isDestroyed) return@run
            val rows = CoreJson.list(json, MeetingSummary::from).getOrNull().orEmpty().map(HomeRows::meeting)
            if (rows.isEmpty()) return@run InkDialog.info(this, "Recent meetings", "No meetings yet. Tap ＋ New to record one.")
            val labels = rows.map { "${it.title}\n${it.stamp}  ·  ${it.chip}" }.toTypedArray()
            AlertDialog.Builder(this, R.style.InkDialog).setTitle("Recent meetings")
                .setItems(labels) { _, i -> openMeeting(rows[i].id, null) }
                .setNegativeButton("CLOSE", null).show()
        }
    }

    /** "CALENDAR": Settings → Calendar; with no link set, the how-to (F18-FR14). */
    private fun openCalendarSettings() {
        val a = agenda
        if (a != null && !a.hasLinks) return InkDialog.info(this, "Add a calendar link", CalendarFormat.HOW_TO + "\n\n" + CalendarFormat.LIMIT)
        startActivity(Intent(this, SettingsActivity::class.java).putExtra(SettingsActivity.EXTRA_SECTION, SettingsActivity.SECTION_CALENDAR))
        overridePendingTransition(0, 0)
    }

    /** F2-FR8: with two mics, tapping the mic line switches to the other and remembers it by name. */
    private fun switchMic() {
        if (rec.busy) return // the mic can't change mid-meeting
        val next = pick().toggled() ?: return
        micChoice = next; render()
        CoreJobs.run({ NativeBridge.nativeSetSettings(SettingsPatch.mic(next)) }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { if (!isDestroyed) { micChoice = it.mic; saveFolder = it.saveFolder; render() } }
        }
    }

    /** The mic a new meeting would use now, or why there is none (F2-FR2). */
    private fun pick(): MicPick {
        val am = getSystemService(AudioManager::class.java)
        return MicSelector.pick(usbInputs(am), bt?.snapshot() ?: BtSnapshot.NONE, micChoice)
    }

    /** ＋ New starts the service; ■ Stop asks it to stop. The hub reports what happened. */
    private fun toggle() {
        if (rec.busy) {
            if (rec !is RecState.Stopping) RecordingService.stop(this)
            return
        }
        val mic = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        when (access.onNew(mic, Environment.isExternalStorageManager())) {
            Permissions.OnNew.GRANT_STORAGE -> { pendingEvent = null; return grantStorage() }
            Permissions.OnNew.ASK_MIC -> return requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), MIC_REQUEST)
            Permissions.OnNew.MIC_BLOCKED -> { pendingEvent = null; return InkDialog.error(this, "The microphone isn't allowed", Permissions.MIC_BLOCKED_TEXT) }
            Permissions.OnNew.START -> Unit
        }
        val p = pick()
        val event = pendingEvent
        pendingEvent = null
        if (!p.canRecord) { pendingTemplate = null; return render() }
        RecordingService.start(this, micChoice, event, nearLive)
    }

    private fun render() {
        val r = rec
        val p = pick()
        dateText.text = CalendarFormat.headerDate(LocalDate.now())
        // Idle: white pill (inkread's Open button). Recording: solid black (its Open → button).
        ui.stylePill(button, if (r.busy) "■  Stop" else "＋  New", filled = r.busy)
        if (!r.busy && !p.canRecord) button.setTextColor(ui.textMuted) // F2-AC1: disabled, reason in the mic line
        val canSwitch = !r.busy && p.toggled() != null
        micLine.text = when (r) {
            is RecState.Starting -> "${r.mic} · connecting the microphone…"
            is RecState.Recording -> "Recording · ${elapsed(r.startElapsedMs)}"
            RecState.Stopping -> "Saving…"
            is RecState.Idle -> r.message ?: listOfNotNull(
                p.chosen?.label ?: p.reason,
                if (canSwitch) "tap to switch microphone" else null,
            ).joinToString("  ·  ")
        }
    }

    private fun elapsed(startElapsedMs: Long): String {
        val s = (SystemClock.elapsedRealtime() - startElapsedMs) / 1000
        return "%d:%02d:%02d".format(s / 3600, (s / 60) % 60, s % 60)
    }

    /** S1: the system's all-files screen, only from a tap (the card's pill, or ＋ New). */
    private fun grantStorage() {
        startActivity(Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName")))
    }

    /** The mic prompt came from ＋ New: granted, so carry on and start the meeting. */
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != MIC_REQUEST) return
        val granted = grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        access.micResult(granted)
        if (granted) toggle() else { pendingEvent = null; pendingTemplate = null; render() }
    }

    companion object {
        private const val RECENT = 5
        private const val MIC_REQUEST = 1
        /** Search's event sheet: record this event key (F18-FR8). */
        const val EXTRA_RECORD_EVENT = "record_event"
        const val EXTRA_TEMPLATE = "template"
        /** Process-wide, so the denial count survives the activity being rebuilt. */
        private val access = Permissions()
    }
}
