package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.Gravity
import android.widget.LinearLayout

/**
 * Template list (spec F11-FR5) in the Inkwell style: every template with a Built-in /
 * Customised / Yours chip and a Default chip; tap one to edit. Mock-up over [MockTemplates].
 */
class TemplatesActivity : Activity() {

    private lateinit var ui: Inkwell

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
    }

    override fun onResume() {
        super.onResume()
        render()
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun render() {
        val column = ui.column().apply {
            addView(ui.header("Settings"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body("Templates", 30f, bold = true).apply { gravity = Gravity.CENTER })
            addView(ui.note("A template sets what the summary asks for and how the transcript file reads.", 14f).apply {
                gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, 0)
            })
            addView(ui.spacer(ui.dim(28)))
            addView(ui.card(padV = ui.dim(6)).apply {
                MockTemplates.all.forEachIndexed { i, t ->
                    if (i > 0) addView(ui.hairline())
                    addView(LinearLayout(this@TemplatesActivity).apply {
                        orientation = LinearLayout.HORIZONTAL
                        gravity = Gravity.CENTER_VERTICAL
                        setPadding(0, ui.dim(14), 0, ui.dim(14))
                        isClickable = true
                        setOnClickListener {
                            startActivity(Intent(this@TemplatesActivity, TemplateEditorActivity::class.java)
                                .putExtra(TemplateEditorActivity.EXTRA_ID, t.id))
                            overridePendingTransition(0, 0)
                        }
                        addView(LinearLayout(this@TemplatesActivity).apply {
                            orientation = LinearLayout.VERTICAL
                            addView(ui.body(t.name, 18f, bold = true))
                            addView(ui.note(t.description, 13f), ui.wrap().apply { topMargin = ui.dim(3) })
                            addView(LinearLayout(this@TemplatesActivity).apply {
                                orientation = LinearLayout.HORIZONTAL
                                addView(ui.chip(t.state, filled = t.state == "Yours"))
                                if (t.id == MockSettings.defaultTemplate) {
                                    addView(ui.chip("Default", filled = true), ui.wrap().apply { marginStart = ui.dim(8) })
                                }
                            }, ui.wrap().apply { topMargin = ui.dim(8) })
                        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                        addView(ui.body("→", 18f).apply { setPadding(ui.dim(16), 0, 0, 0) })
                    })
                }
            })
            addView(ui.spacer(ui.dim(24)))
            addView(ui.pill("＋  New template", size = 17f) {
                val n = MockTemplate("mine-${MockTemplates.all.size}", "My template", "Describe when to use it.", "Yours", "",
                    MockTemplates.byId("general")!!.instructions)
                MockTemplates.all.add(n)
                startActivity(Intent(this@TemplatesActivity, TemplateEditorActivity::class.java)
                    .putExtra(TemplateEditorActivity.EXTRA_ID, n.id))
                overridePendingTransition(0, 0)
            }, ui.wrap())
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
}
