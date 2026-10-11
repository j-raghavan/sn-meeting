package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Intent
import android.media.AudioManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CalendarFormat
import dev.jraghavan.snmeeting.model.CalendarLink
import dev.jraghavan.snmeeting.model.CalendarPatch
import dev.jraghavan.snmeeting.model.CalendarPrefs
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.IndexStatus
import dev.jraghavan.snmeeting.model.SearchModel
import dev.jraghavan.snmeeting.model.ProviderRows
import dev.jraghavan.snmeeting.model.SaveFolders
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.RatesView
import dev.jraghavan.snmeeting.model.Reprice
import dev.jraghavan.snmeeting.model.Spend
import dev.jraghavan.snmeeting.model.SpendReport
import dev.jraghavan.snmeeting.model.TemplateChoice
import dev.jraghavan.snmeeting.model.TemplateRow
import dev.jraghavan.snmeeting.model.TestResult
import dev.jraghavan.snmeeting.record.BluetoothState
import dev.jraghavan.snmeeting.record.BtSnapshot
import dev.jraghavan.snmeeting.record.MicKind
import dev.jraghavan.snmeeting.record.MicSelector
import dev.jraghavan.snmeeting.record.builtInInputs
import dev.jraghavan.snmeeting.record.usbInputs
import dev.jraghavan.snmeeting.update.UpdateController
import dev.jraghavan.snmeeting.update.UpdateGate
import dev.jraghavan.snmeeting.update.UpdatePolicy

/**
 * Settings (spec F10) in the Inkwell style (F9): microphone, save folder, the transcribe and
 * summarise providers with Test connection, and templates. The core owns settings.json: each
 * choice is a merge patch through nativeSetSettings, saved at once (no Save button), and the
 * page shows the core's reply.
 */
class SettingsActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    private val handler = Handler(Looper.getMainLooper())
    private var page: ScrollView? = null

    /** The core's settings, or null until they arrive. */
    private var settings: SettingsView? = null
    /** The core's last refusal, by settings field ("" for none), shown under its section. */
    private val errors = mutableMapOf<String, String>()
    private var bt: BluetoothState? = null
    /** The Custom… folder field is open. */
    private var editingFolder = false
    private var templates: List<TemplateRow> = emptyList()
    /** F16-FR5: the last 12 months of spend and the rates in force; null until the core answers. */
    private var spend: SpendReport? = null
    private var rates: RatesView? = null

    /**
     * The last Test connection result per "role/provider" (F10-FR5: shown until the next test),
     * and the view currently showing each. render() rebuilds every view, so results are kept
     * here and re-applied, and a late result is written to whichever view is current.
     */
    private val results = mutableMapOf<String, String>()
    private val resultViews = mutableMapOf<String, TextView>()
    private val modelViews = mutableMapOf<String, TextView>()

    /** settings.debug.segmentMs; the rows show once set, or after a long-press on the version. */
    /** F19-FR5: the Rebuild search index line ("Indexing 120/340", then the result). */
    private var indexLine: String? = null
    private var indexView: TextView? = null
    /** F20-FR10: the "Tag older meetings…" line, kept across redraws. */
    private var tagView: TextView? = null
    private var tagLine: String? = null
    private var rebuilding = false
    private val indexPoll = object : Runnable {
        override fun run() {
            if (!rebuilding) return
            SearchJobs.query({ NativeBridge.nativeIndexStatus() }) { json ->
                if (isDestroyed || !rebuilding) return@query
                val line = CoreJson.obj(json, IndexStatus::from).getOrNull()?.let(SearchModel::indexing) ?: return@query
                if (line != indexLine) { indexLine = line; indexView?.text = line } // the line only, no redraw
            }
            handler.postDelayed(this, 1000)
        }
    }
    private var segmentMs: Long? = null
    private var showSegmentLength = false

    /** F18-FR13: the calendar's settings and links (by name and elided URL, never the URL). */
    private var calendar = CalendarPrefs()
    private var links: List<CalendarLink>? = null
    private var refreshing = false
    /** Home's "CALENDAR" opens this screen at the Calendar card. */
    private var calendarCard: View? = null
    private var scrollToCalendar = false

    /** F14-AC2: whether this copy carries the release key (read once; a cheap PackageManager call). */
    private val canUpdate by lazy { UpdateController(this).canUpdateItself() }
    private var unobserveUpdates: (() -> Unit)? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        CoreJobs.init(filesDir.absolutePath)
        SearchJobs.init(filesDir.absolutePath)
        CalendarJobs.init(filesDir.absolutePath)
        scrollToCalendar = intent.getStringExtra(EXTRA_SECTION) == SECTION_CALENDAR
        render()
    }

    override fun onStart() {
        super.onStart()
        // Mics come and go while the screen is open (F10-FR2).
        bt = BluetoothState(this, handler) { render() }.also { it.open() }
        // F14-FR8: a check or download finishing redraws About with its result.
        unobserveUpdates = UpdateGate.observe { render() }
        if (rebuilding) handler.postDelayed(indexPoll, 1000)
    }

    override fun onStop() {
        handler.removeCallbacks(indexPoll) // the rebuild carries on; onStart polls again
        unobserveUpdates?.invoke(); unobserveUpdates = null
        bt?.close(); bt = null
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        // Key files may have been added and templates changed in Templates or on the desktop.
        // CoreJobs answers in order: the templates are in hand when the settings arrive.
        scope.core({ NativeBridge.nativeListTemplates() }) { json ->
            CoreJson.list(json, TemplateRow::from).getOrNull()?.let { templates = it }
        }
        scope.core({ NativeBridge.nativeGetSettings() }) { applySettings(it) }
        loadSpend()
        loadLinks() // the links file may have changed on the desktop (F18-FR1)
    }

    /** F16-FR5: spend by month and the rates' date; one redraw when both are in. */
    private fun loadSpend() {
        scope.core({ NativeBridge.nativeGetRates() }) { json -> rates = CoreJson.obj(json, RatesView::from).getOrNull() }
        scope.core({
            val (from, to) = Spend.lastMonths(java.time.YearMonth.now())
            NativeBridge.nativeSpend(from, to)
        }) { json -> spend = CoreJson.obj(json, SpendReport::from).getOrNull(); render() }
    }

    /**
     * F16-FR5 / FR6: "rates as of", the last 12 months by provider (transcription and minutes),
     * what the totals leave out, Edit rates, and Re-price with current rates.
     */
    private fun spendSection(column: LinearLayout) {
        val report = spend ?: return
        ui.section(column, "Spend").apply {
            rates?.let { r ->
                addView(ui.note("Rates as of ${r.asOf}", 13f).apply { setPadding(0, ui.dim(10), 0, ui.dim(4)) })
                r.problems.forEach { addView(error(it)) }
            }
            val rows = report.rows()
            if (rows.isEmpty()) addView(ui.note("Nothing spent yet", 15f).apply { setPadding(0, ui.dim(10), 0, ui.dim(10)) })
            rows.forEachIndexed { i, row ->
                if (i > 0) addView(ui.hairline())
                addView(LinearLayout(this@SettingsActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(0, ui.dim(10), 0, ui.dim(10))
                    addView(LinearLayout(this@SettingsActivity).apply {
                        orientation = LinearLayout.HORIZONTAL
                        gravity = Gravity.CENTER_VERTICAL
                        addView(ui.body(row.month, 16f), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                        addView(ui.body(row.total, 16f, bold = row.total != "—"))
                    })
                    row.lines.forEach { addView(ui.note(it, 13f), ui.wrap().apply { topMargin = ui.dim(3) }) }
                })
            }
            addView(ui.hairline())
            val notes = report.notes + if (report.unpriced > 0) listOf("“+” means a call has no rate yet; add one, then re-price.") else emptyList()
            notes.forEach { addView(ui.note(it, 13f, ui.textMuted), ui.wrap().apply { topMargin = ui.dim(6) }) }
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(12))
                addView(ui.pill("Edit rates  →", size = 15f) {
                    startActivity(Intent(this@SettingsActivity, RatesActivity::class.java)); overridePendingTransition(0, 0)
                })
                addView(ui.pill("Re-price with current rates", size = 15f) { reprice() }, ui.wrap().apply { marginStart = ui.dim(12) })
            })
        }
    }

    /** F16-FR6: a dry run says what would change; only a confirmation rewrites stored costs. */
    private fun reprice() {
        scope.core({ NativeBridge.nativeRepriceAll(true) }) { json ->
            when (val r = CoreJson.obj(json, Reprice::from)) {
                is CoreResult.Err -> InkDialog.error(this, "Couldn't re-price", r.message)
                is CoreResult.Ok -> {
                    val q = r.value.question()
                        ?: return@core InkDialog.error(this, "Nothing to re-price", "Every meeting's cost already matches the current rates.")
                    InkDialog.confirm(this, "Re-price with current rates?", q, "Re-price") {
                        scope.core({ NativeBridge.nativeRepriceAll(false) }) { done ->
                            val e = CoreJson.ok(done)
                            if (e is CoreResult.Err) InkDialog.error(this, "Couldn't re-price", e.message)
                            loadSpend()
                        }
                    }
                }
            }
        }
    }

    /** nativeGetSettings / nativeSetSettings reply: the new settings, or a refusal by field. */
    private fun applySettings(json: String) {
        when (val r = CoreJson.obj(json, SettingsView::from)) {
            is CoreResult.Ok -> {
                if (r.value.saveFolder != settings?.saveFolder) editingFolder = false
                settings = r.value; errors.clear()
                segmentMs = r.value.segmentMsDebug
                CoreJson.obj(json) { o -> CalendarPrefs.from(o.optJSONObject("settings")?.optJSONObject("calendar")) }.getOrNull()?.let { calendar = it }
            }
            is CoreResult.Err -> errors[r.field ?: ""] = r.message
        }
        render()
    }

    /** One change, saved at once. */
    private fun patch(json: String) = scope.core({ NativeBridge.nativeSetSettings(json) }) { applySettings(it) }

    /** F2-FR5 / F2-AC3: one-minute segments, to check rollover on the device. */
    private fun segmentLength(card: LinearLayout) {
        card.addView(ui.hairline())
        card.addView(ui.eyebrow("Segment length").apply { setPadding(0, ui.dim(12), 0, 0) })
        card.choice("Standard", "15 minutes, or less for a large file", segmentMs == null) { setSegmentMs(null) }
        card.addView(ui.hairline())
        card.choice("Short (1 min)", "For checking how recordings are split", segmentMs != null) { setSegmentMs(SHORT_SEGMENT_MS) }
    }

    private fun setSegmentMs(ms: Long?) {
        segmentMs = ms
        patch(SettingsPatch.segmentMs(ms))
    }

    /** F10-FR2: Automatic, or a specific mic; the same choice as tapping Home's mic card. */
    private fun micSection(column: LinearLayout) {
        val s = settings
        ui.section(column, "Microphone").apply {
            if (s == null) return@apply addView(ui.note("Reading settings…", 13f).apply { setPadding(0, ui.dim(10), 0, ui.dim(10)) })
            val am = getSystemService(AudioManager::class.java)
            val options = MicSelector.pick(usbInputs(am), bt?.snapshot() ?: BtSnapshot.NONE, s.mic, builtInInputs(am)).options
            MicSelector.settingsRows(options, s.mic).forEachIndexed { i, row ->
                if (i > 0) addView(ui.hairline())
                choice(row.label, row.hint, row.selected) { patch(SettingsPatch.mic(row.choice)) }
            }
            if (options.none { it.kind == MicKind.BLUETOOTH }) {
                addView(ui.note("Paired Bluetooth headsets appear here when connected.", 13f).apply {
                    setPadding(0, ui.dim(4), 0, ui.dim(10))
                })
            }
            errors["mic"]?.let { addView(error(it)) }
        }
    }

    /**
     * F15 / ADR D14: which recorder a new meeting uses. Standard (the default) is Android's
     * recorder, transcribed after Stop; Near-live (new) records through its own encoder and
     * writes the transcript while recording. A meeting under way keeps the recorder it began with.
     */
    private fun recorderSection(column: LinearLayout) {
        val s = settings ?: return
        ui.section(column, "Recorder").apply {
            choice("Standard", "Transcribe after you stop", !s.nearLive) { patch(SettingsPatch.recorder(SettingsView.STANDARD)) }
            addView(ui.hairline())
            choice("Near-live (new)", "The transcript fills in while you record, about twice a minute", s.nearLive) {
                patch(SettingsPatch.recorder(SettingsView.NEAR_LIVE))
            }
            errors["recorder"]?.let { addView(error(it)) }
        }
    }

    /**
     * F17-FR2 (OQ13): the notes tool bar docks right (the default) or left, never top.
     */
    private fun notesSection(column: LinearLayout) {
        val n = settings?.notes ?: return
        ui.section(column, "Notes").apply {
            choice("Tool bar on the right", null, n.dock == dev.jraghavan.snmeeting.notes.NotesPrefs.Dock.RIGHT) {
                patch(dev.jraghavan.snmeeting.notes.NotesPatch.dock(dev.jraghavan.snmeeting.notes.NotesPrefs.Dock.RIGHT))
            }
            addView(ui.hairline())
            choice("Tool bar on the left", null, n.dock == dev.jraghavan.snmeeting.notes.NotesPrefs.Dock.LEFT) {
                patch(dev.jraghavan.snmeeting.notes.NotesPatch.dock(dev.jraghavan.snmeeting.notes.NotesPrefs.Dock.LEFT))
            }
            addView(ui.hairline())
            // F17-FR9 / OQ20: on by default; the regions of the canvas go with the Stop call.
            choice("Use my notes for the minutes", "At Stop, images of your notes go with the transcript, so the minutes and Search can use your handwriting",
                n.useForMinutes) { patch(dev.jraghavan.snmeeting.notes.NotesPatch.useForMinutes(!n.useForMinutes)) }
            errors["notes.dock"]?.let { addView(error(it)) }
        }
    }

    /**
     * F10-FR3: EXPORT/sn-meeting, Document/Meetings or a folder typed relative to internal
     * storage. The core validates it (inside storage, writable, not a keys or templates folder)
     * and its reason shows under the field.
     */
    private fun saveSection(column: LinearLayout) {
        val s = settings ?: return
        ui.section(column, "Save to").apply {
            SaveFolders.PRESETS.forEachIndexed { i, p ->
                if (i > 0) addView(ui.hairline())
                choice(p.path, p.hint, s.saveFolder == p.path) { editingFolder = false; patch(SettingsPatch.saveFolder(p.path)) }
            }
            addView(ui.hairline())
            val custom = SaveFolders.isCustom(s.saveFolder)
            // Always opens the field, even when a custom folder is already chosen.
            addView(ui.choiceRow("Custom…", if (custom) s.saveFolder else "A folder inside internal storage", custom || editingFolder) {
                editingFolder = true; render()
            })
            if (editingFolder) {
                val field = ui.field(if (custom) s.saveFolder else "")
                addView(field, ui.fullWidth().apply { topMargin = ui.dim(4) })
                addView(ui.note("Relative to internal storage, e.g. Document/Work meetings", 12f, ui.textMuted), ui.wrap().apply { topMargin = ui.dim(4) })
                addView(ui.pill("Use this folder", size = 15f) {
                    val path = field.text.toString().trim()
                    if (path.isNotEmpty()) patch(SettingsPatch.saveFolder(path))
                }, ui.wrap().apply { topMargin = ui.dim(10); bottomMargin = ui.dim(6) })
            }
            errors["saveFolder"]?.let { addView(error(it)) }
            val note = listOfNotNull("Applies to new meetings.", SaveFolders.previous(s.previousFolders)).joinToString(" ")
            addView(ui.note(note, 13f).apply { setPadding(0, ui.dim(4), 0, ui.dim(10)) })
        }
    }

    /** F10-FR6: the default template for new transcriptions, and the way to manage them (F11). */
    private fun templateSection(column: LinearLayout) {
        val s = settings ?: return
        ui.section(column, "Templates").apply {
            addView(ui.note("Default for new transcriptions", 13f).apply { setPadding(0, ui.dim(10), 0, 0) })
            TemplateChoice.of(null, s.effectiveDefaultTemplate, templates).options.forEachIndexed { i, t ->
                if (i > 0) addView(ui.hairline())
                choice(t.name, t.description, t.id == s.effectiveDefaultTemplate) { patch(SettingsPatch.defaultTemplate(t.id)) }
            }
            errors["defaultTemplate"]?.let { addView(error(it)) }
            addView(ui.hairline())
            addView(ui.pill("Manage templates  →", size = 16f) {
                startActivity(Intent(this@SettingsActivity, TemplatesActivity::class.java)); overridePendingTransition(0, 0)
            }, ui.wrap().apply { topMargin = ui.dim(12); bottomMargin = ui.dim(12) })
        }
    }

    /**
     * F14-FR8, inkread's Settings → Updates in the About card: "Check for updates", the result in
     * place, Install / Skip this version / Later when one is found, and the two toggles.
     */
    private fun updates(card: LinearLayout) {
        val prefs = settings?.updates ?: return
        card.addView(ui.hairline())
        if (!canUpdate) {
            card.addView(ui.note(UpdatePolicy.CANT_UPDATE, 13f).apply { setPadding(0, ui.dim(12), 0, ui.dim(12)) })
        } else {
            val st = UpdateGate.state
            val busy = st == UpdateGate.State.Checking || st is UpdateGate.State.Downloading
            card.addView(LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(12), 0, ui.dim(8))
                addView(LinearLayout(this@SettingsActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(ui.eyebrow("Updates"))
                    addView(ui.body("Check for updates", 16f), ui.wrap().apply { topMargin = ui.dim(4) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.pill(if (busy) "Checking…" else "Check now", size = 15f) {
                    if (!busy) UpdateGate.checkNow(this@SettingsActivity, prefs)
                })
            })
            when (st) {
                UpdateGate.State.Idle -> Unit
                UpdateGate.State.Checking -> card.addView(updateLine("Checking GitHub for a newer release…"))
                is UpdateGate.State.Downloading -> card.addView(updateLine("Downloading and verifying v${st.version}…"))
                is UpdateGate.State.Line -> card.addView(updateLine(st.text))
                is UpdateGate.State.Found -> found(card, st.available)
            }
        }
        card.addView(ui.hairline())
        card.toggle("Check for updates automatically",
            "When Meetings opens, at most every 6 hours, never while recording or transcribing. There is no store to deliver fixes.",
            prefs.autoCheck) { patch(SettingsPatch.autoCheck(!prefs.autoCheck)) }
        card.addView(ui.hairline())
        card.toggle("Install updates automatically",
            "Download and verify a new release and go straight to Android's install confirmation, without asking first. Off by default.",
            prefs.autoInstall) { patch(SettingsPatch.autoInstall(!prefs.autoInstall)) }
    }

    private fun updateLine(text: String): TextView = ui.note(text, 13f).apply { setPadding(0, 0, 0, ui.dim(12)) }

    /** A found release: its version and notes, then Install / Skip this version / Later. */
    private fun found(card: LinearLayout, a: UpdateController.Available) {
        card.addView(ui.body("Meetings v${a.version} is available", 16f, bold = true), ui.wrap().apply { topMargin = ui.dim(4) })
        card.addView(ui.note(UpdatePolicy.notes(a), 13f), ui.wrap().apply { topMargin = ui.dim(6) })
        card.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, ui.dim(14), 0, ui.dim(14))
            addView(ui.pill("Install", filled = true, size = 15f) { UpdateGate.install(this@SettingsActivity, a) })
            addView(ui.pill("Skip this version", size = 15f) { UpdateGate.skip(a) }, ui.wrap().apply { marginStart = ui.dim(12) })
            addView(ui.pill("Later", size = 15f) { UpdateGate.later() }, ui.wrap().apply { marginStart = ui.dim(12) })
        })
    }

    /** A keyline checkbox row (the toggle style of inkread's Settings), saved at once. */
    private fun LinearLayout.toggle(label: String, hint: String, on: Boolean, flip: () -> Unit) {
        addView(LinearLayout(this@SettingsActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, ui.dim(12), 0, ui.dim(12))
            addView(ui.checkbox(on))
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(ui.dim(14), 0, 0, 0)
                addView(ui.body(label, 17f))
                addView(ui.note(hint, 13f), ui.wrap().apply { topMargin = ui.dim(2) })
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            isClickable = true
            setOnClickListener { flip() }
        })
    }

    /** The core's reason a change was refused, under the field it concerns. */
    private fun error(message: String): TextView = ui.note(message, 13f, ui.ink).apply { setPadding(0, ui.dim(4), 0, ui.dim(10)) }

    /** Rebuilds the page, keeping the scroll position (one redraw per change). */
    private fun show(column: LinearLayout) {
        val y = page?.scrollY ?: 0
        val p = ui.page(column)
        page = p
        setContentView(p)
        val target = calendarCard
        if (scrollToCalendar && target != null && settings != null) {
            scrollToCalendar = false
            p.post { p.scrollTo(0, (target.top - ui.dim(40)).coerceAtLeast(0)) }
        } else if (y > 0) p.post { p.scrollTo(0, y) }
    }

    /** F18-FR13: each link's name, elided URL and status, as the core reports them. */
    private fun loadLinks() {
        scope.core({ NativeBridge.nativeCalendarLinks() }) { json ->
            CoreJson.list(json, CalendarLink::from).getOrNull()?.let { if (it != links) { links = it; render() } }
        }
    }

    /** Refresh now: every link, then the list again (F18-FR4). */
    private fun refreshLinks() {
        if (refreshing) return
        refreshing = true; render()
        CalendarJobs.refresh(true) { _ ->
            refreshing = false
            if (!isDestroyed) { render(); loadLinks() }
        }
    }

    /**
     * F18-FR13: Settings → CALENDAR. The links (added from the computer, read-only here), Refresh
     * now, the refresh interval, linking by time, sending the event with the minutes (OQ25), the
     * template keyword rules, and how to add a link.
     */
    private fun calendarSection(column: LinearLayout) {
        if (settings == null) return
        val card = ui.section(column, "Calendar")
        calendarCard = card
        val zone = java.time.ZoneId.systemDefault()
        val l = links
        card.apply {
            when {
                l == null -> addView(ui.note("Reading the calendar links…", 13f).apply { setPadding(0, ui.dim(10), 0, ui.dim(10)) })
                l.isEmpty() -> addView(ui.body("No calendar link yet.", 16f).apply { setPadding(0, ui.dim(10), 0, ui.dim(4)) })
                else -> l.forEachIndexed { i, link ->
                    if (i > 0) addView(ui.hairline())
                    addView(LinearLayout(this@SettingsActivity).apply {
                        orientation = LinearLayout.VERTICAL
                        setPadding(0, ui.dim(10), 0, ui.dim(10))
                        addView(ui.body(link.name, 17f, bold = true))
                        addView(ui.eyebrow(link.elided, ui.textMuted).apply { isAllCaps = false; text = link.elided }, ui.wrap().apply { topMargin = ui.dim(2) })
                        addView(ui.note(CalendarFormat.linkStatus(link, zone), 13f), ui.wrap().apply { topMargin = ui.dim(4) })
                    })
                }
            }
            if (!l.isNullOrEmpty()) {
                addView(ui.pill(if (refreshing) "Refreshing…" else "Refresh now", size = 15f) { refreshLinks() },
                    ui.wrap().apply { topMargin = ui.dim(6); bottomMargin = ui.dim(10) })
            }
            addView(ui.hairline())
            addView(ui.eyebrow("Refresh").apply { setPadding(0, ui.dim(12), 0, 0) })
            CalendarFormat.INTERVALS.forEachIndexed { i, (min, label) ->
                if (i > 0) addView(ui.hairline())
                choice(label, if (min == 0) "Only when you tap Refresh or UPDATED on Home" else null, calendar.refreshMin == min) {
                    patch(CalendarPatch.refreshMin(min))
                }
            }
            errors["calendar.refreshMin"]?.let { addView(error(it)) }
            addView(ui.hairline())
            toggle("Link recordings to events by time", "A recording started during an event (or 5 minutes before) is linked to it.", calendar.linkByTime) {
                patch(CalendarPatch.linkByTime(!calendar.linkByTime))
            }
            addView(ui.hairline())
            toggle("Send the event with the minutes", "The title, time, attendees' names and agenda go to the summary provider at Stop.", calendar.eventInMinutes) {
                patch(CalendarPatch.eventInMinutes(!calendar.eventInMinutes))
            }
            addView(ui.hairline())
            addView(ui.eyebrow("Template by keyword").apply { setPadding(0, ui.dim(12), 0, 0) })
            addView(ui.note("The first keyword found in an event's title picks its template; otherwise the default.", 13f).apply { setPadding(0, ui.dim(4), 0, ui.dim(6)) })
            calendar.rules.forEachIndexed { i, (keyword, tid) ->
                addView(LinearLayout(this@SettingsActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                    setPadding(0, ui.dim(8), 0, ui.dim(8))
                    val name = templates.firstOrNull { it.id == tid }?.name ?: tid
                    addView(ui.body("“$keyword”  →  $name", 16f), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                    addView(ui.eyebrow("Remove", ui.textMuted).apply {
                        setPadding(ui.dim(12), ui.dim(6), 0, ui.dim(6)); isClickable = true
                        setOnClickListener { patch(CalendarPatch.rules(calendar.rules.filterIndexed { j, _ -> j != i })) }
                    })
                })
            }
            errors["calendar.templateRules"]?.let { addView(error(it)) }
            addView(ui.pill("Add a rule", size = 15f) { addRule() }, ui.wrap().apply { topMargin = ui.dim(6); bottomMargin = ui.dim(12) })
            addView(ui.hairline())
            addView(ui.eyebrow("Adding a link").apply { setPadding(0, ui.dim(12), 0, 0) })
            addView(ui.note(CalendarFormat.HOW_TO, 13f).apply { setPadding(0, ui.dim(6), 0, 0); setLineSpacing(0f, 1.2f) })
            addView(ui.note(CalendarFormat.LIMIT, 13f).apply { setPadding(0, ui.dim(6), 0, 0) })
            addView(ui.note(CalendarFormat.SYNC_NOTE, 13f, ui.textMuted).apply { setPadding(0, ui.dim(6), 0, ui.dim(12)) })
        }
    }

    /** A keyword, then the template it picks; saved as the whole list (F18-FR13). */
    private fun addRule() {
        InkDialog.text(this, ui, "Keyword in the event's title", "") { keyword ->
            val k = keyword.trim()
            if (k.isEmpty()) return@text
            val rows = templates
            if (rows.isEmpty()) return@text
            android.app.AlertDialog.Builder(this, R.style.InkDialog).setTitle("Template for “$k”")
                .setItems(rows.map { it.name }.toTypedArray()) { _, i -> patch(CalendarPatch.rules(calendar.rules + (k to rows[i].id))) }
                .setNegativeButton("CANCEL", null).show()
        }
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** Rebuilds the page after each choice; e-ink redraws once per tap, never animates. */
    private fun render() {
        resultViews.clear(); modelViews.clear(); calendarCard = null
        val column = ui.column().apply {
            addView(ui.header("Back"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body("Settings", 30f, bold = true).apply { gravity = Gravity.CENTER })
        }

        micSection(column)
        recorderSection(column)
        notesSection(column)
        errors[""]?.let { column.addView(error(it)) }

        saveSection(column)

        providerSection(column, "Transcription", SettingsView.TRANSCRIBE,
            "Anthropic can't transcribe audio, so it is offered for summaries only.")
        providerSection(column, "Summary & to-dos", SettingsView.SUMMARISE, null)

        templateSection(column)

        calendarSection(column)

        spendSection(column)

        ui.section(column, "About").apply {
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(0, ui.dim(10), 0, ui.dim(10))
                // Long-press reveals the segment length used by the rollover check (F2-AC3).
                addView(ui.body("Meetings v${ui.versionName()}", 16f).apply {
                    setOnLongClickListener { showSegmentLength = true; render(); true }
                })
                addView(ui.note("Core v${coreVersion()}", 13f), ui.wrap().apply { topMargin = ui.dim(2) })
                addView(ui.note("Keys are read from MyStyle/SnCopilot (shared with SN-Copilot).", 13f), ui.wrap().apply { topMargin = ui.dim(4) })
                addView(ui.note("Your templates live in MyStyle/Meetings/templates.", 13f), ui.wrap().apply { topMargin = ui.dim(2) })
            })
            updates(this)
            searchIndex(this)
            tagOlder(this)
            if (showSegmentLength || segmentMs != null) segmentLength(this)
        }

        column.addView(ui.spacer(ui.dim(36)))
        column.addView(ui.closingMark())
        show(column)
    }

    /**
     * F19-FR5: "Rebuild search index" in About. The index is a cache of the meeting files, so
     * this loses nothing; while it runs the line reads "Indexing 120/340", and Search answers
     * from what is indexed so far.
     */
    private fun searchIndex(card: LinearLayout) {
        card.addView(ui.hairline())
        card.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, ui.dim(12), 0, ui.dim(12))
            addView(ui.pill("Rebuild search index", size = 15f) { rebuildIndex() }, ui.wrap())
            val note = ui.note(indexLine ?: "Search reads a copy of your meetings kept on this device; rebuilding it loses nothing.", 13f)
            indexView = note
            addView(note, ui.wrap().apply { topMargin = ui.dim(8) })
        })
    }

    /** F20-FR10 / OQ31: "Tag older meetings…": the count and cost first, then one call each. */
    private fun tagOlder(card: LinearLayout) {
        card.addView(ui.hairline())
        card.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, ui.dim(12), 0, ui.dim(12))
            addView(ui.pill("Tag older meetings…", size = 15f) { askTagOlder() }, ui.wrap())
            val note = ui.note(tagLine ?: dev.jraghavan.snmeeting.model.TagOlder.NOTE, 13f)
            tagView = note
            addView(note, ui.wrap().apply { topMargin = ui.dim(8) })
        })
    }

    private fun askTagOlder() {
        if (LongJobs.lane.isRunning(TAG_OLDER)) return
        scope.core({ NativeBridge.nativeTagOlder(true) }) { json ->
            when (val r = CoreJson.obj(json, dev.jraghavan.snmeeting.model.TagOlder::from)) {
                is CoreResult.Err -> InkDialog.error(this, "Can't tag older meetings", r.message)
                is CoreResult.Ok -> {
                    val q = r.value.question()
                        ?: return@core InkDialog.info(this, "Nothing to tag", "Every summarised meeting already has tags.")
                    InkDialog.confirm(this, "Tag older meetings?", q, "Tag them") {
                        tagLine = "Tagging…"; tagView?.text = tagLine
                        LongJobs.lane.run(TAG_OLDER, { NativeBridge.nativeTagOlder(false) }) { done ->
                            tagLine = when (val d = CoreJson.obj(done) { it }) {
                                is CoreResult.Ok -> dev.jraghavan.snmeeting.model.TagOlder.result(d.value)
                                is CoreResult.Err -> "Couldn't tag older meetings: ${d.message}"
                            }
                            if (!isDestroyed) tagView?.text = tagLine
                            SearchJobs.refresh()
                        }
                    }
                }
            }
        }
    }

    private fun rebuildIndex() {
        if (rebuilding) return
        rebuilding = true
        indexLine = "Still indexing…"; indexView?.text = indexLine
        handler.postDelayed(indexPoll, 1000)
        SearchJobs.index({ NativeBridge.nativeRebuildIndex() }) { json ->
            rebuilding = false
            handler.removeCallbacks(indexPoll)
            if (isDestroyed) return@index
            indexLine = SearchModel.rebuilt(json); indexView?.text = indexLine
        }
    }

    /**
     * F10-FR5 / F1-FR9: Test connection lists models with the key (no audio, no cost) on the
     * job lane, one test per role at a time, so it never waits behind or delays a short call.
     * The result stays on screen until the next test; it is never saved and never holds a key.
     */
    private fun testConnection(role: String, provider: String) {
        val key = "$role/$provider"
        val dir = filesDir.absolutePath
        val started = LongJobs.lane.run("${JobLane.TEST_PREFIX}$role", {
            // The process can restart straight into Settings; nativeInit is idempotent and cheap.
            val init = NativeBridge.nativeInit(dir)
            if (CoreJson.ok(init) is CoreResult.Err) init else NativeBridge.nativeTestConnection(role, provider)
        }) { json ->
            if (!isDestroyed) {
                showResult(key, TestResult.line(json))
                refreshModels()
            }
        }
        if (started) showResult(key, "Checking…")
    }

    /**
     * D2: a test lists the account's models, so the Model lines can name the pick. Only those
     * lines change: a full redraw would drop a half-typed override or folder.
     */
    private fun refreshModels() {
        scope.core({ NativeBridge.nativeGetSettings() }) { json ->
            val s = CoreJson.obj(json, SettingsView::from).getOrNull() ?: return@core
            settings = s
            for ((role, view) in modelViews) view.text = modelLine(role, s)
        }
    }

    private fun modelLine(role: String, s: SettingsView) = ProviderRows.model(role, s) ?: "None until a key file is added"

    private fun showResult(key: String, text: String) {
        results[key] = text
        resultViews[key]?.text = text
    }

    /** The Rust core's version (F1-FR5); a cheap call, so it runs on the UI thread. */
    private fun coreVersion(): String = try {
        NativeBridge.nativeVersion()
    } catch (t: Throwable) {
        "unavailable (${t.javaClass.simpleName})" // libmeeting.so missing or not loadable
    }

    private fun LinearLayout.choice(label: String, hint: String?, selected: Boolean, enabled: Boolean = true, pick: () -> Unit) {
        addView(ui.choiceRow(label, hint, selected, enabled) { if (!selected) pick(); render() })
    }

    /**
     * F10-FR4: a radio per provider with its key status (one without a key can't be chosen),
     * the model that will run and an optional override (empty = the key file's or the
     * default), and Test connection (F10-FR5).
     */
    private fun providerSection(column: LinearLayout, title: String, role: String, footnote: String?) {
        val s = settings ?: return
        ui.section(column, title).apply {
            ProviderRows.of(role, s).forEachIndexed { i, row ->
                if (i > 0) addView(ui.hairline())
                choice(row.name, row.hint, row.selected, enabled = row.enabled || row.selected) { patch(SettingsPatch.provider(role, row.id)) }
            }
            errors["$role.provider"]?.let { addView(error(it)) }
            addView(ui.hairline())
            val current = s.effective(role).provider
            val resultKey = "$role/$current"
            val result: TextView = ui.note(results[resultKey] ?: "", 13f).apply { setPadding(0, 0, 0, ui.dim(6)) }
            resultViews[resultKey] = result
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(12), 0, ui.dim(8))
                addView(LinearLayout(this@SettingsActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(ui.eyebrow("Model"))
                    addView(ui.body(modelLine(role, s), 16f).also { modelViews[role] = it }, ui.wrap().apply { topMargin = ui.dim(4) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                // F10-FR5: the core lists models with the key (no audio, no cost).
                addView(ui.pill("Test connection", size = 15f) { testConnection(role, current) })
            })
            addView(result)
            val field = ui.field(s.choice(role).model ?: "").apply { hint = "Use the key file's model" }
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(4), 0, ui.dim(10))
                addView(field, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.pill("Set", size = 15f) { patch(SettingsPatch.model(role, field.text.toString())) },
                    ui.wrap().apply { marginStart = ui.dim(12) })
            })
            errors["$role.model"]?.let { addView(error(it)) }
            if (footnote != null) addView(ui.note(footnote, 13f, ui.textMuted).apply { setPadding(0, 0, 0, ui.dim(10)) })
        }
    }

    companion object {
        private const val SHORT_SEGMENT_MS = 60_000L
        /** F20-FR10: "Tag older meetings…" on the job lane (one run at a time). */
        private const val TAG_OLDER = "tags:older"
        /** Open at a card: Home's "CALENDAR" asks for [SECTION_CALENDAR]. */
        const val EXTRA_SECTION = "section"
        const val SECTION_CALENDAR = "calendar"
    }
}
