package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.Gravity
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.TemplateOrigin
import dev.jraghavan.snmeeting.model.TemplateRow

/**
 * Template list (spec F11-FR5) in the Inkwell style, read from the core: every template with a
 * Built-in / Customised / Yours chip, a Default chip, and the problem with the user's file if
 * there is one; tap one to edit. Re-read on resume, so desktop edits show (F11-FR3).
 */
class TemplatesActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    private var default: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        CoreJobs.init(filesDir.absolutePath)
    }

    override fun onResume() {
        super.onResume()
        // In queue order: the default is known when the list arrives (one redraw).
        scope.core({ NativeBridge.nativeGetSettings() }) { json ->
            CoreJson.obj(json, SettingsView::from).getOrNull()?.let { default = it.effectiveDefaultTemplate }
        }
        scope.core({ NativeBridge.nativeListTemplates() }) { json ->
            when (val r = CoreJson.list(json, TemplateRow::from)) {
                is CoreResult.Ok -> render(r.value, null)
                is CoreResult.Err -> render(emptyList(), r.message)
            }
        }
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun edit(extras: Intent.() -> Unit) {
        startActivity(Intent(this, TemplateEditorActivity::class.java).apply(extras))
        overridePendingTransition(0, 0)
    }

    private fun render(rows: List<TemplateRow>, error: String?) {
        val column = ui.column().apply {
            addView(ui.header("Settings"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body("Templates", 30f, bold = true).apply { gravity = Gravity.CENTER })
            addView(ui.note("A template sets what the summary asks for and how the transcript file reads.", 14f).apply {
                gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, 0)
            })
            addView(ui.spacer(ui.dim(28)))
            addView(ui.card(padV = ui.dim(6)).apply {
                if (error != null) {
                    addView(ui.note("Couldn't read the templates: $error").apply { setPadding(0, ui.dim(14), 0, ui.dim(14)) })
                }
                rows.forEachIndexed { i, t ->
                    if (i > 0) addView(ui.hairline())
                    addView(row(t))
                }
            })
            addView(ui.spacer(ui.dim(24)))
            addView(ui.pill("＋  New template", size = 17f) { edit { putExtra(TemplateEditorActivity.EXTRA_NEW, true) } }, ui.wrap())
            addView(ui.spacer(ui.dim(16)))
            addView(ui.note("Edit them here, or as files in MyStyle/Meetings/templates on your computer.", 13f, ui.textMuted).apply {
                gravity = Gravity.CENTER
            })
            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
            addView(ui.closingMark())
        }
        setContentView(ui.page(column))
    }

    private fun row(t: TemplateRow) = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, ui.dim(14), 0, ui.dim(14))
        isClickable = true
        setOnClickListener { edit { putExtra(TemplateEditorActivity.EXTRA_ID, t.id) } }
        addView(LinearLayout(this@TemplatesActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(ui.body(t.name, 18f, bold = true))
            addView(ui.note(t.description, 13f), ui.wrap().apply { topMargin = ui.dim(3) })
            // F11-AC3: a broken file from the desktop says so, and what is used instead.
            t.problem?.let { addView(ui.note(it, 13f, ui.ink), ui.wrap().apply { topMargin = ui.dim(4) }) }
            addView(LinearLayout(this@TemplatesActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                addView(ui.chip(t.origin.label, filled = t.origin == TemplateOrigin.YOURS))
                if (t.id == default) addView(ui.chip("Default", filled = true), ui.wrap().apply { marginStart = ui.dim(8) })
            }, ui.wrap().apply { topMargin = ui.dim(8) })
        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        addView(ui.body("→", 18f).apply { setPadding(ui.dim(16), 0, 0, 0) })
    }
}
