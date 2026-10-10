package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.RateEntry
import dev.jraghavan.snmeeting.model.RatesEdit
import dev.jraghavan.snmeeting.model.RatesView

/**
 * Settings → SPEND → Edit rates (spec F16-FR4) in the Inkwell style: every rate in force, with
 * its source date, editable prices (USD per minute, or per 1M tokens), Save and Reset to
 * defaults. The core validates and dates each edit; stored costs never change here (F16-FR6).
 */
class RatesActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var scope: ScreenScope
    private var page: ScrollView? = null
    private var rates: RatesView? = null
    /** The fields on screen, by entry index and field name. */
    private val fields = mutableMapOf<Pair<Int, String>, EditText>()
    private var message: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        scope = ScreenScope(this)
        CoreJobs.init(filesDir.absolutePath)
        render()
    }

    override fun onResume() {
        super.onResume()
        scope.core({ NativeBridge.nativeGetRates() }) { apply(it, null) }
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    /** A nativeGetRates / nativeSetRates reply: the rates, or the core's reason, then one redraw. */
    private fun apply(json: String, saved: String?) {
        when (val r = CoreJson.obj(json, RatesView::from)) {
            is CoreResult.Ok -> { rates = r.value; message = saved }
            is CoreResult.Err -> message = r.message
        }
        render()
    }

    private fun save() {
        val entries = rates?.entries ?: return
        val typed = fields.mapValues { it.value.text.toString() }
        val (json, bad) = RatesEdit.build(entries, typed)
        if (json == null) { message = bad; return render() }
        scope.core({ NativeBridge.nativeSetRates(json) }) {
            apply(it, "Saved. Costs already recorded keep their price until you re-price them in Settings.")
        }
    }

    private fun reset() {
        InkDialog.confirm(this, "Reset to the default rates?", "Your edited rates are removed. Costs already recorded don't change.", "Reset") {
            scope.core({ NativeBridge.nativeSetRates(RatesEdit.RESET) }) { apply(it, "Back to the default rates.") }
        }
    }

    private fun render() {
        fields.clear()
        val r = rates
        val column = ui.column().apply {
            addView(ui.header("Settings"))
            addView(ui.spacer(ui.dim(28)))
            addView(ui.body("Rates", 30f, bold = true).apply { gravity = Gravity.CENTER })
            addView(ui.note("USD per minute of audio, or per million tokens. Rates are never fetched: check them against each provider's pricing page.", 14f).apply {
                gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, 0)
            })
            if (r == null) {
                addView(ui.note(message ?: "Reading rates…", 13f).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(24), 0, 0) })
                return@apply
            }
            addView(ui.eyebrow("Rates as of ${r.asOf}").apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(14), 0, 0) }, ui.fullWidth())
            r.problems.forEach { addView(ui.note(it, 13f, ui.ink).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(6), 0, 0) }, ui.fullWidth()) }
            addView(ui.spacer(ui.dim(24)))
            addView(ui.card(padV = ui.dim(6)).apply {
                r.entries.forEachIndexed { i, e ->
                    if (i > 0) addView(ui.hairline())
                    addView(entry(i, e))
                }
            })
            message?.let { addView(ui.note(it, 13f, ui.ink).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(14), 0, 0) }, ui.fullWidth()) }
            addView(LinearLayout(this@RatesActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
                setPadding(0, ui.dim(20), 0, 0)
                addView(ui.pill("Save rates", filled = true, size = 16f) { save() })
                addView(ui.pill("Reset to defaults", size = 16f) { reset() }, ui.wrap().apply { marginStart = ui.dim(12) })
            }, ui.fullWidth())
            addView(ui.spacer(ui.dim(36)))
            addView(ui.closingMark())
        }
        val y = page?.scrollY ?: 0
        val p = ui.page(column)
        page = p
        setContentView(p)
        if (y > 0) p.post { p.scrollTo(0, y) }
    }

    /** One rate: its model and source, then a labelled field per price of its unit. */
    private fun entry(i: Int, e: RateEntry) = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, ui.dim(14), 0, ui.dim(14))
        addView(LinearLayout(this@RatesActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.body(e.title, 17f, bold = true), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.chip(if (e.user) "Yours" else "Default", filled = e.user))
        })
        addView(ui.note("As of ${e.asOf}", 12f, ui.textMuted), ui.wrap().apply { topMargin = ui.dim(2) })
        e.note?.let { addView(ui.note(it, 13f, ui.ink), ui.wrap().apply { topMargin = ui.dim(4) }) }
        for ((field, label) in e.fields) {
            val f = ui.field(e.price(field)?.let(::plain) ?: "").apply {
                inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
                hint = "—"
            }
            fields[i to field] = f
            addView(LinearLayout(this@RatesActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(8), 0, 0)
                addView(ui.note(label, 14f, ui.inkSoft), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(f, LinearLayout.LayoutParams(ui.dim(200), LinearLayout.LayoutParams.WRAP_CONTENT))
            })
        }
    }

    /** 0.006, not 6.0E-3. */
    private fun plain(v: Double): String = java.math.BigDecimal.valueOf(v).stripTrailingZeros().toPlainString()
}
