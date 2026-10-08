package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView

/**
 * Template editor (spec F11-FR5) in the Inkwell style: plain text fields, Preview, Save,
 * Use as default, Reset / Duplicate / Delete. Mock-up: edits stay in memory, Preview shows a
 * sample render, and nothing is written to MyStyle/Meetings/templates yet.
 */
class TemplateEditorActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var template: MockTemplate
    private lateinit var status: TextView
    private lateinit var preview: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        template = MockTemplates.byId(intent.getStringExtra(EXTRA_ID)) ?: run { finish(); return }
        render()
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun render() {
        val name = ui.field(template.name)
        val description = ui.field(template.description)
        val vocabulary = ui.field(template.vocabulary)
        val instructions = ui.field(template.instructions, lines = 8)
        val layout = ui.field(MockTemplates.STANDARD_LAYOUT, lines = 12, mono = true)
        status = ui.note("", 14f).apply { gravity = Gravity.CENTER }
        preview = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; visibility = View.GONE }

        val column = ui.column().apply {
            addView(ui.header("Templates"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body(template.name, 30f, bold = true).apply { gravity = Gravity.CENTER })
            addView(LinearLayout(this@TemplateEditorActivity).apply {
                gravity = Gravity.CENTER
                addView(ui.chip(template.state, filled = template.state == "Yours"))
                if (template.id == MockSettings.defaultTemplate) addView(ui.chip("Default", filled = true), ui.wrap().apply { marginStart = ui.dim(8) })
            }, ui.fullWidth().apply { topMargin = ui.dim(10) })

            addView(labelled("Name", name))
            addView(labelled("Description", description))
            addView(labelled("Vocabulary", vocabulary, "Names and terms to help transcription, separated by commas."))
            addView(labelled("Instructions", instructions, "What the summary should contain. Sent to the AI as its instructions."))
            addView(labelled("Layout", layout, "How the transcript file reads. Placeholders: {{ title }}, {{ summary }}, {{ todos }}, {{ transcript }} …"))

            addView(ui.spacer(ui.dim(24)))
            addView(LinearLayout(this@TemplateEditorActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                addView(ui.pill("Preview", size = 16f) {
                    preview.visibility = if (preview.visibility == View.VISIBLE) View.GONE else View.VISIBLE
                })
                addView(ui.pill("Save", filled = true, size = 16f) {
                    template.name = name.text.toString()
                    template.description = description.text.toString()
                    template.vocabulary = vocabulary.text.toString()
                    template.instructions = instructions.text.toString()
                    if (template.state == "Built-in") template.state = "Customised"
                    status.text = "Saved as your copy: MyStyle/Meetings/templates/${template.id}.md"
                }, ui.wrap().apply { marginStart = ui.dim(14) })
                if (template.id != MockSettings.defaultTemplate) {
                    addView(ui.pill("Use as default", size = 16f) {
                        MockSettings.defaultTemplate = template.id; render()
                    }, ui.wrap().apply { marginStart = ui.dim(14) })
                }
            }, ui.fullWidth())
            addView(status, ui.fullWidth().apply { topMargin = ui.dim(12) })

            // Preview: the layout rendered against a sample meeting (F11-AC6).
            preview.addView(ui.spacer(ui.dim(20)))
            preview.addView(ui.eyebrowItalic("Preview with a sample meeting"))
            preview.addView(ui.spacer(ui.dim(10)))
            preview.addView(ui.card().apply {
                addView(TextView(this@TemplateEditorActivity).apply {
                    text = MockTemplates.SAMPLE_RENDER; setTextColor(ui.ink); textSize = ui.fs(13f)
                    typeface = ui.mono; setLineSpacing(0f, 1.3f)
                })
            })
            addView(preview, ui.fullWidth())

            addView(ui.spacer(ui.dim(28)))
            addView(LinearLayout(this@TemplateEditorActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                val links = mutableListOf<Pair<String, () -> Unit>>()
                if (template.state == "Customised") links += "Reset to built-in" to {
                    template.state = "Built-in"; status.text = "Your copy was removed; the built-in is back."
                }
                links += "Duplicate" to { status.text = "A copy named \"${template.name} (copy)\" would be created." }
                if (template.state == "Yours") links += "Delete" to { MockTemplates.all.remove(template); finish() }
                links.forEachIndexed { i, (label, action) ->
                    addView(ui.eyebrow(label, ui.textMuted).apply {
                        isClickable = true; setOnClickListener { action() }
                        setPadding(ui.dim(14), ui.dim(8), ui.dim(14), ui.dim(8))
                    })
                    if (i < links.size - 1) addView(ui.eyebrow("·", ui.textMuted))
                }
            }, ui.fullWidth())

            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(24)))
            addView(ui.note("File: MyStyle/Meetings/templates/${template.id}.md — you can also edit it on your computer.", 12f, ui.textMuted).apply {
                gravity = Gravity.CENTER
            })
            addView(ui.spacer(ui.dim(16)))
            addView(ui.closingMark())
        }
        setContentView(ui.page(column))
    }

    /** Mono eyebrow over a field, with an optional italic hint underneath. */
    private fun labelled(label: String, field: View, hint: String? = null): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        addView(ui.spacer(ui.dim(22)))
        addView(ui.eyebrow(label))
        addView(field, ui.fullWidth().apply { topMargin = ui.dim(8) })
        if (hint != null) addView(ui.note(hint, 12f, ui.textMuted), ui.wrap().apply { topMargin = ui.dim(4) })
        layoutParams = ui.fullWidth()
    }

    companion object {
        const val EXTRA_ID = "template_id"
    }
}
