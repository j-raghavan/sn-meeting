package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.TemplateActions
import dev.jraghavan.snmeeting.model.TemplateDraft
import dev.jraghavan.snmeeting.model.TemplateOrigin
import dev.jraghavan.snmeeting.model.TemplateRow
import dev.jraghavan.snmeeting.model.str

/**
 * Template editor (spec F11-FR5) in the Inkwell style: plain text fields for name,
 * description, vocabulary, instructions and layout; Preview renders the layout against the
 * sample meeting; Save validates in the core and writes the user copy, with "Line N:" errors
 * under the field. Heavier editing is expected on the desktop; the footer says where the file is.
 */
class TemplateEditorActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    /** The template as last read or saved; null for a new one until it is saved. */
    private var saved: TemplateRow? = null
    private var draft: TemplateDraft? = null
    private var ids: List<String> = emptyList()
    private var default: String? = null

    private val fields = mutableMapOf<String, EditText>()
    private val fieldErrors = mutableMapOf<String, TextView>()
    private lateinit var status: TextView
    private lateinit var preview: LinearLayout
    private lateinit var previewText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        CoreJobs.init(filesDir.absolutePath)
        val id = intent.getStringExtra(EXTRA_ID)
        val isNew = intent.getBooleanExtra(EXTRA_NEW, false)
        if (id == null && !isNew) return finish()
        setContentView(ui.page(ui.column().apply { addView(ui.header("Templates")) }))
        // In queue order: the ids (for a new template's id) and the default, then the template.
        scope.core({ NativeBridge.nativeListTemplates() }) { json ->
            ids = CoreJson.list(json, TemplateRow::from).getOrNull()?.map { it.id } ?: emptyList()
        }
        scope.core({ NativeBridge.nativeGetSettings() }) { json ->
            default = CoreJson.obj(json, SettingsView::from).getOrNull()?.effectiveDefaultTemplate
        }
        // A new template starts from General's instructions and layout.
        scope.core({ NativeBridge.nativeGetTemplate(id ?: GENERAL) }) { json ->
            when (val r = CoreJson.obj(json, TemplateRow::from)) {
                is CoreResult.Ok -> if (isNew) {
                    draft = TemplateDraft.of(r.value).copy(id = "", name = "My template", description = "", vocabulary = "", sections = emptyList())
                    render()
                } else {
                    saved = r.value; draft = TemplateDraft.of(r.value); render()
                }
                is CoreResult.Err -> { InkDialog.error(this, "Couldn't open the template", r.message); finish() }
            }
        }
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** The fields as typed. A new template gets its id from its name when first saved. */
    private fun current(): TemplateDraft {
        val d = draft!!
        fun f(k: String, old: String) = fields[k]?.text?.toString() ?: old
        val name = f("name", d.name)
        val id = saved?.id ?: TemplateDraft.slug(name, ids)
        return d.copy(id = id, name = name, description = f("description", d.description), vocabulary = f("vocabulary", d.vocabulary),
            instructions = f("instructions", d.instructions), layout = f("layout", d.layout))
    }

    private fun render() {
        val d = draft ?: return
        val t = saved
        fields.clear(); fieldErrors.clear()
        status = ui.note("", 14f).apply { gravity = Gravity.CENTER }
        previewText = TextView(this).apply { setTextColor(ui.ink); textSize = ui.fs(13f); typeface = ui.mono; setLineSpacing(0f, 1.3f) }
        preview = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL; visibility = View.GONE
            addView(ui.spacer(ui.dim(20)))
            addView(ui.eyebrowItalic("Preview with a sample meeting"))
            addView(ui.spacer(ui.dim(10)))
            addView(ui.card().apply { addView(previewText) })
        }

        val column = ui.column().apply {
            addView(ui.header("Templates"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body(t?.name ?: "New template", 30f, bold = true).apply { gravity = Gravity.CENTER })
            addView(LinearLayout(this@TemplateEditorActivity).apply {
                gravity = Gravity.CENTER
                addView(ui.chip((t?.origin ?: TemplateOrigin.YOURS).label, filled = t == null || t.origin == TemplateOrigin.YOURS))
                if (t != null && t.id == default) addView(ui.chip("Default", filled = true), ui.wrap().apply { marginStart = ui.dim(8) })
            }, ui.fullWidth().apply { topMargin = ui.dim(10) })
            t?.problem?.let { addView(ui.note(it, 14f, ui.ink).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, 0) }, ui.fullWidth()) }

            addView(labelled("name", "Name", ui.field(d.name)))
            addView(labelled("description", "Description", ui.field(d.description)))
            addView(labelled("vocabulary", "Vocabulary", ui.field(d.vocabulary), "Names and terms to help transcription, separated by commas."))
            addView(labelled("instructions", "Instructions", ui.field(d.instructions, lines = 8), "What the summary should contain. Sent to the AI as its instructions."))
            addView(labelled("layout", "Layout", ui.field(d.layout, lines = 12, mono = true),
                "How the transcript file reads. Placeholders: {{ title }}, {{ summary }}, {{ todos }}, {{ transcript }} …"))

            addView(ui.spacer(ui.dim(24)))
            addView(LinearLayout(this@TemplateEditorActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                addView(ui.pill("Preview", size = 16f) { preview() })
                addView(ui.pill("Save", filled = true, size = 16f) { save() }, ui.wrap().apply { marginStart = ui.dim(14) })
                if (t != null && t.id != default && t.problem == null) {
                    addView(ui.pill("Use as default", size = 16f) { useAsDefault(t.id) }, ui.wrap().apply { marginStart = ui.dim(14) })
                }
            }, ui.fullWidth())
            addView(status, ui.fullWidth().apply { topMargin = ui.dim(12) })
            addView(preview, ui.fullWidth())
            if (t != null) {
                addView(ui.spacer(ui.dim(28)))
                addView(links(t), ui.fullWidth())
            }

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(24)))
            val file = "MyStyle/Meetings/templates/${t?.id ?: "<id>"}.md"
            addView(ui.note("File: $file — you can also edit it on your computer.", 12f, ui.textMuted).apply { gravity = Gravity.CENTER })
            addView(ui.spacer(ui.dim(16)))
            addView(ui.closingMark())
        }
        setContentView(ui.page(column))
    }

    /** Mono eyebrow over a field, its error (from the core) and an optional italic hint. */
    private fun labelled(key: String, label: String, field: EditText, hint: String? = null): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        fields[key] = field
        val err = ui.note("", 13f, ui.ink).apply { visibility = View.GONE }
        fieldErrors[key] = err
        addView(ui.spacer(ui.dim(22)))
        addView(ui.eyebrow(label))
        addView(field, ui.fullWidth().apply { topMargin = ui.dim(8) })
        addView(err, ui.wrap().apply { topMargin = ui.dim(4) })
        if (hint != null) addView(ui.note(hint, 12f, ui.textMuted), ui.wrap().apply { topMargin = ui.dim(4) })
        layoutParams = ui.fullWidth()
    }

    /** Shows the core's refusal under its field (or in the status line), clearing the others. */
    private fun showError(e: CoreResult.Err?) {
        fieldErrors.values.forEach { it.visibility = View.GONE }
        val under = e?.field?.let { fieldErrors[it] }
        if (e != null && under != null) {
            under.text = TemplateDraft.errorText(e); under.visibility = View.VISIBLE
        }
        if (e != null) status.text = if (under != null) "Not saved: see ${e.field} above." else TemplateDraft.errorText(e)
    }

    /** F11-AC6: the layout against the sample meeting. Nothing is saved. */
    private fun preview() {
        val json = current().json()
        scope.core({ NativeBridge.nativePreviewTemplate(json) }) { reply ->
            when (val r = CoreJson.obj(reply) { it.str("markdown") }) {
                is CoreResult.Ok -> {
                    showError(null); status.text = ""
                    previewText.text = r.value; preview.visibility = View.VISIBLE
                }
                is CoreResult.Err -> { showError(r); preview.visibility = View.GONE }
            }
        }
    }

    /** Validates and writes the user copy; a built-in becomes Customised. */
    private fun save() {
        val d = current()
        scope.core({ NativeBridge.nativeSaveTemplate(d.json()) }) { reply ->
            when (val r = CoreJson.obj(reply, TemplateRow::from)) {
                is CoreResult.Ok -> {
                    saved = r.value; draft = TemplateDraft.of(r.value)
                    if (r.value.id !in ids) ids = ids + r.value.id
                    render()
                    status.text = "Saved to MyStyle/Meetings/templates/${r.value.id}.md"
                }
                is CoreResult.Err -> showError(r)
            }
        }
    }

    /** Reset to built-in · Duplicate · Delete, as muted mono links (F11-FR5). */
    private fun links(t: TemplateRow): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER
        val a = TemplateActions.of(t)
        val links = mutableListOf<Pair<String, () -> Unit>>()
        if (a.reset) links += "Reset to built-in" to { reset(t) }
        links += "Duplicate" to { duplicate() }
        if (a.delete) links += "Delete" to { delete(t) }
        links.forEachIndexed { i, (label, action) ->
            addView(ui.eyebrow(label, ui.textMuted).apply {
                isClickable = true; setOnClickListener { action() }
                setPadding(ui.dim(14), ui.dim(8), ui.dim(14), ui.dim(8))
            })
            if (i < links.size - 1) addView(ui.eyebrow("·", ui.textMuted))
        }
    }

    /** F11-AC4: deletes the user copy; the built-in text is back. */
    private fun reset(t: TemplateRow) {
        InkDialog.confirm(this, "Reset ${t.name}?", "Your copy in MyStyle/Meetings/templates is deleted and the built-in text is used again.", "Reset") {
            scope.core({ NativeBridge.nativeResetTemplate(t.id) }) { reply ->
                (CoreJson.ok(reply) as? CoreResult.Err)?.let { return@core InkDialog.error(this, "Couldn't reset it", it.message) }
                scope.core({ NativeBridge.nativeGetTemplate(t.id) }) { json ->
                    CoreJson.obj(json, TemplateRow::from).getOrNull()?.let { saved = it; draft = TemplateDraft.of(it); render() }
                    status.text = "Your copy was removed; the built-in is back."
                }
            }
        }
    }

    /** A new user template from this one, as typed, under a new id; the editor moves to it. */
    private fun duplicate() {
        val name = TemplateActions.copyName(current().name)
        val copy = current().copy(id = TemplateDraft.slug(name, ids), name = name)
        scope.core({ NativeBridge.nativeSaveTemplate(copy.json()) }) { reply ->
            when (val r = CoreJson.obj(reply, TemplateRow::from)) {
                is CoreResult.Ok -> {
                    saved = r.value; draft = TemplateDraft.of(r.value); ids = ids + r.value.id
                    render()
                    status.text = "Duplicated as ${r.value.name}"
                }
                is CoreResult.Err -> showError(r)
            }
        }
    }

    /** A YOURS template: its file is deleted (nativeResetTemplate deletes a user-only template). */
    private fun delete(t: TemplateRow) {
        InkDialog.confirm(this, "Delete ${t.name}?", "Its file in MyStyle/Meetings/templates is deleted. Meetings that used it use General meeting.", "Delete") {
            scope.core({ NativeBridge.nativeResetTemplate(t.id) }) { reply ->
                when (val r = CoreJson.ok(reply)) {
                    is CoreResult.Ok -> finish()
                    is CoreResult.Err -> InkDialog.error(this, "Couldn't delete it", r.message)
                }
            }
        }
    }

    private fun useAsDefault(templateId: String) {
        scope.core({ NativeBridge.nativeSetSettings(SettingsPatch.defaultTemplate(templateId)) }) { reply ->
            when (val r = CoreJson.obj(reply, SettingsView::from)) {
                is CoreResult.Ok -> { default = r.value.effectiveDefaultTemplate; draft = current(); render() }
                is CoreResult.Err -> status.text = r.message
            }
        }
    }

    companion object {
        const val EXTRA_ID = "template_id"
        const val EXTRA_NEW = "template_new"
        private const val GENERAL = "general"
    }
}
