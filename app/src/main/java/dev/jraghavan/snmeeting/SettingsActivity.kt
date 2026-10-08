package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Intent
import android.media.AudioManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.ProviderRows
import dev.jraghavan.snmeeting.model.SaveFolders
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.TemplateChoice
import dev.jraghavan.snmeeting.model.TemplateRow
import dev.jraghavan.snmeeting.model.TestResult
import dev.jraghavan.snmeeting.record.BluetoothState
import dev.jraghavan.snmeeting.record.BtSnapshot
import dev.jraghavan.snmeeting.record.MicKind
import dev.jraghavan.snmeeting.record.MicSelector
import dev.jraghavan.snmeeting.record.usbInputs

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

    /**
     * The last Test connection result per "role/provider" (F10-FR5: shown until the next test),
     * and the view currently showing each. render() rebuilds every view, so results are kept
     * here and re-applied, and a late result is written to whichever view is current.
     */
    private val results = mutableMapOf<String, String>()
    private val resultViews = mutableMapOf<String, TextView>()

    /** settings.debug.segmentMs; the rows show once set, or after a long-press on the version. */
    private var segmentMs: Long? = null
    private var showSegmentLength = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        CoreJobs.init(filesDir.absolutePath)
        render()
    }

    override fun onStart() {
        super.onStart()
        // Mics come and go while the screen is open (F10-FR2).
        bt = BluetoothState(this, handler) { render() }.also { it.open() }
    }

    override fun onStop() {
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
    }

    /** nativeGetSettings / nativeSetSettings reply: the new settings, or a refusal by field. */
    private fun applySettings(json: String) {
        when (val r = CoreJson.obj(json, SettingsView::from)) {
            is CoreResult.Ok -> {
                if (r.value.saveFolder != settings?.saveFolder) editingFolder = false
                settings = r.value; errors.clear()
                segmentMs = r.value.segmentMsDebug
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
            val options = MicSelector.pick(usbInputs(am), bt?.snapshot(am) ?: BtSnapshot.NONE, s.mic).options
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

    /** The core's reason a change was refused, under the field it concerns. */
    private fun error(message: String): TextView = ui.note(message, 13f, ui.ink).apply { setPadding(0, ui.dim(4), 0, ui.dim(10)) }

    /** Rebuilds the page, keeping the scroll position (one redraw per change). */
    private fun show(column: LinearLayout) {
        val y = page?.scrollY ?: 0
        val p = ui.page(column)
        page = p
        setContentView(p)
        if (y > 0) p.post { p.scrollTo(0, y) }
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** Rebuilds the page after each choice; e-ink redraws once per tap, never animates. */
    private fun render() {
        resultViews.clear()
        val column = ui.column().apply {
            addView(ui.header("Back"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body("Settings", 30f, bold = true).apply { gravity = Gravity.CENTER })
        }

        micSection(column)
        errors[""]?.let { column.addView(error(it)) }

        saveSection(column)

        providerSection(column, "Transcription", SettingsView.TRANSCRIBE,
            "Anthropic can't transcribe audio, so it is offered for summaries only.")
        providerSection(column, "Summary & to-dos", SettingsView.SUMMARISE, null)

        templateSection(column)

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
            if (showSegmentLength || segmentMs != null) segmentLength(this)
        }

        column.addView(ui.spacer(ui.dim(36)))
        column.addView(ui.closingMark())
        show(column)
    }

    /**
     * F10-FR5 / F1-FR9: Test connection lists models with the key (no audio, no cost) on the
     * job lane, one test per role at a time, so it never waits behind or delays a short call.
     * The result stays on screen until the next test; it is never saved and never holds a key.
     */
    private fun testConnection(role: String, provider: String) {
        val key = "$role/$provider"
        val dir = filesDir.absolutePath
        val started = LongJobs.lane.run("test:$role", {
            // The process can restart straight into Settings; nativeInit is idempotent and cheap.
            val init = NativeBridge.nativeInit(dir)
            if (CoreJson.ok(init) is CoreResult.Err) init else NativeBridge.nativeTestConnection(role, provider)
        }) { json -> if (!isDestroyed) showResult(key, TestResult.line(json)) }
        if (started) showResult(key, "Checking…")
    }

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

    /** One provider role: a radio per provider with its key status, the model, and Test connection (F10-FR4/FR5). */
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
                    addView(ui.body(ProviderRows.model(role, s) ?: "None until a key file is added", 16f), ui.wrap().apply { topMargin = ui.dim(4) })
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
    }
}
