package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONObject

/**
 * Settings (spec F10) in the Inkwell style (F9): microphone, save folder, the transcribe and
 * summarise providers with Test connection, and templates. Choices still live in [MockSettings]
 * (memory only) until the core owns settings.json; Test connection is real and runs in the core.
 */
class SettingsActivity : Activity() {

    private lateinit var ui: Inkwell

    /**
     * The last Test connection result per "role/provider" (F10-FR5: shown until the next test),
     * and the view currently showing each. render() rebuilds every view, so results are kept
     * here and re-applied, and a late result is written to whichever view is current.
     */
    private val results = mutableMapOf<String, String>()
    private val resultViews = mutableMapOf<String, TextView>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        render()
    }

    override fun onResume() {
        super.onResume()
        render() // the default template may have changed in Templates
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

        ui.section(column, "Microphone").apply {
            choice("Automatic", "USB first, then Bluetooth", MockSettings.mic == "auto") { MockSettings.mic = "auto" }
            addView(ui.hairline())
            choice("USB — USB-Audio - USB Audio", "Connected now", MockSettings.mic == "usb") { MockSettings.mic = "usb" }
            addView(ui.hairline())
            addView(ui.note("Paired Bluetooth headsets appear here when connected.", 13f).apply {
                setPadding(0, ui.dim(10), 0, ui.dim(10))
            })
        }

        ui.section(column, "Save to").apply {
            choice("EXPORT/sn-meeting", "Synced to your computer by Supernote Partner", MockSettings.folder == "EXPORT/sn-meeting") {
                MockSettings.folder = "EXPORT/sn-meeting"
            }
            addView(ui.hairline())
            choice("Document/Meetings", null, MockSettings.folder == "Document/Meetings") { MockSettings.folder = "Document/Meetings" }
            addView(ui.hairline())
            choice("Custom…", "A folder inside internal storage", MockSettings.folder == "custom") { MockSettings.folder = "custom" }
            addView(ui.note("Applies to new meetings. Older meetings stay where they are and stay listed.", 13f).apply {
                setPadding(0, ui.dim(4), 0, ui.dim(10))
            })
        }

        providerSection(column, "Transcription", MockSettings.transcribeChoices, MockSettings.transcribe,
            "Anthropic can't transcribe audio, so it is offered for summaries only.") { MockSettings.transcribe = it }
        providerSection(column, "Summary & to-dos", MockSettings.summariseChoices, MockSettings.summarise, null) {
            MockSettings.summarise = it
        }

        ui.section(column, "Templates").apply {
            val def = MockTemplates.byId(MockSettings.defaultTemplate)
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(0, ui.dim(10), 0, ui.dim(10))
                addView(ui.note("Default for new transcriptions", 13f))
                addView(ui.body(def?.name ?: "General meeting", 18f, bold = true), ui.wrap().apply { topMargin = ui.dim(2) })
                addView(ui.note(def?.description ?: "", 13f), ui.wrap().apply { topMargin = ui.dim(2) })
            })
            addView(ui.pill("Manage templates  →", size = 16f) {
                startActivity(Intent(this@SettingsActivity, TemplatesActivity::class.java)); overridePendingTransition(0, 0)
            }, ui.wrap().apply { bottomMargin = ui.dim(12) })
        }

        ui.section(column, "About").apply {
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(0, ui.dim(10), 0, ui.dim(10))
                addView(ui.body("Meetings v${ui.versionName()}", 16f))
                addView(ui.note("Core v${coreVersion()}", 13f), ui.wrap().apply { topMargin = ui.dim(2) })
                addView(ui.note("Keys are read from MyStyle/SnCopilot (shared with SN-Copilot).", 13f), ui.wrap().apply { topMargin = ui.dim(4) })
                addView(ui.note("Your templates live in MyStyle/Meetings/templates.", 13f), ui.wrap().apply { topMargin = ui.dim(2) })
            })
        }

        column.addView(ui.spacer(ui.dim(36)))
        column.addView(ui.closingMark())
        setContentView(ui.page(column))
    }

    /**
     * F10-FR5 / F1-FR9: Test connection runs in the core, off the UI thread. The result's
     * `detail` stays on screen until the next test; it is never saved. The JSON never holds a key.
     */
    private fun testConnection(role: String, provider: String) {
        val key = "$role/$provider"
        showResult(key, "Testing…")
        val dir = filesDir.absolutePath
        CoreJobs.run({
            // The process can restart straight into Settings; nativeInit is idempotent and cheap.
            // If it fails, show why rather than the core's "nativeInit has not run".
            val init = NativeBridge.nativeInit(dir)
            if (isError(init)) init else NativeBridge.nativeTestConnection(role, provider)
        }) { json ->
            Log.i("Meetings", json)
            if (!isDestroyed) showResult(key, resultText(json))
        }
    }

    private fun showResult(key: String, text: String) {
        results[key] = text
        resultViews[key]?.text = text
    }

    private fun isError(json: String): Boolean = try {
        JSONObject(json).has("error")
    } catch (e: Exception) {
        true
    }

    /** The line to show: the result's `detail`, else the core's `error`. */
    private fun resultText(json: String): String = try {
        val o = JSONObject(json)
        o.optString("detail").ifEmpty { o.optString("error").ifEmpty { json } }
    } catch (e: Exception) {
        json
    }

    /** The Rust core's version (F1-FR5); a cheap call, so it runs on the UI thread. */
    private fun coreVersion(): String = try {
        NativeBridge.nativeVersion()
    } catch (t: Throwable) {
        "unavailable (${t.javaClass.simpleName})" // libmeeting.so missing or not loadable
    }

    private fun LinearLayout.choice(label: String, hint: String?, selected: Boolean, enabled: Boolean = true, pick: () -> Unit) {
        addView(ui.choiceRow(label, hint, selected, enabled) { pick(); render() })
    }

    /** One provider role: a radio per provider with its key status, the model, and Test connection (F10-FR4/FR5). */
    private fun providerSection(column: LinearLayout, title: String, choices: List<String>, current: String, footnote: String?, pick: (String) -> Unit) {
        ui.section(column, title).apply {
            choices.forEachIndexed { i, id ->
                val p = MockSettings.provider(id)
                if (i > 0) addView(ui.hairline())
                val hint = if (p.hasKey) "Key found: ${p.keyFile}" else "Add ${p.keyFile} to MyStyle/SnCopilot"
                choice(p.name, hint, current == id, enabled = p.hasKey) { pick(id) }
            }
            addView(ui.hairline())
            val chosen = MockSettings.provider(current)
            val model = if (title == "Transcription") chosen.model else if (chosen.id == "openai") "gpt-4o-mini" else chosen.model
            val role = if (title == "Transcription") "transcribe" else "summarise"
            val resultKey = "$role/$current"
            val result: TextView = ui.note(results[resultKey] ?: "", 13f).apply { setPadding(0, 0, 0, ui.dim(6)) }
            resultViews[resultKey] = result
            addView(LinearLayout(this@SettingsActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(12), 0, ui.dim(12))
                addView(LinearLayout(this@SettingsActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(ui.eyebrow("Model"))
                    addView(ui.body(model, 16f), ui.wrap().apply { topMargin = ui.dim(4) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                // F10-FR5: the core lists models with the key (no audio, no cost).
                addView(ui.pill("Test connection", size = 15f) { testConnection(role, current) })
            })
            addView(result)
            if (footnote != null) addView(ui.note(footnote, 13f, ui.textMuted).apply { setPadding(0, 0, 0, ui.dim(10)) })
        }
    }
}
