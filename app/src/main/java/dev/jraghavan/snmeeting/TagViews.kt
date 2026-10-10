package dev.jraghavan.snmeeting

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.model.Tags

/** Children laid out left to right, wrapping onto new lines (a meeting's tag pills, F20-FR4). */
class FlowRow(context: Context, private val gapX: Int, private val gapY: Int) : ViewGroup(context) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val maxW = MeasureSpec.getSize(widthMeasureSpec) - paddingLeft - paddingRight
        var x = 0; var y = 0; var lineH = 0
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            if (c.visibility == View.GONE) continue
            c.measure(MeasureSpec.makeMeasureSpec(maxW, MeasureSpec.AT_MOST), MeasureSpec.UNSPECIFIED)
            if (x > 0 && x + c.measuredWidth > maxW) { x = 0; y += lineH + gapY; lineH = 0 }
            x += c.measuredWidth + gapX
            lineH = maxOf(lineH, c.measuredHeight)
        }
        setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), y + lineH + paddingTop + paddingBottom)
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val maxW = r - l - paddingLeft - paddingRight
        var x = 0; var y = 0; var lineH = 0
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            if (c.visibility == View.GONE) continue
            if (x > 0 && x + c.measuredWidth > maxW) { x = 0; y += lineH + gapY; lineH = 0 }
            c.layout(paddingLeft + x, paddingTop + y, paddingLeft + x + c.measuredWidth, paddingTop + y + c.measuredHeight)
            x += c.measuredWidth + gapX
            lineH = maxOf(lineH, c.measuredHeight)
        }
    }
}

/** F20-FR4: a meeting's tags as pills, then "＋ tag". A pill's tap asks once, then removes it. */
object TagPills {
    fun row(a: Activity, ui: Inkwell, tags: List<String>, onRemove: (String) -> Unit, onAdd: () -> Unit, onOpen: ((String) -> Unit)? = null): View =
        FlowRow(a, ui.dim(10), ui.dim(10)).apply {
            tags.forEach { t ->
                addView(ui.chip(Tags.shown(t)).apply {
                    setPadding(ui.dim(14), ui.dim(6), ui.dim(14), ui.dim(6))
                    isClickable = true
                    setOnClickListener { InkDialog.confirm(a, "Remove tag", Tags.removeQuestion(t), "Remove") { onRemove(t) } }
                    // F20-FR7: a long press opens the Tag Graph on this tag.
                    onOpen?.let { open -> setOnLongClickListener { open(t); true } }
                })
            }
            addView(ui.chip("＋ tag").apply {
                setPadding(ui.dim(14), ui.dim(6), ui.dim(14), ui.dim(6))
                setTextColor(ui.inkSoft)
                isClickable = true
                setOnClickListener { onAdd() }
            })
        }
}

/**
 * F20-FR4: "＋ tag": typed, or written in a pen strip and read by the summarise provider
 * (F19-FR9's recogniser, ≤ 15 s). The core normalises what comes back (F20-FR2).
 */
object TagEntry {
    fun show(a: Activity, ui: Inkwell, onTag: (String) -> Unit) {
        if (a.isFinishing || a.isDestroyed) return
        val field = ui.field("").apply {
            inputType = InputType.TYPE_CLASS_TEXT
            hint = Tags.ADD_HINT
        }
        val status = ui.note("", 13f, ui.textMuted).apply { visibility = View.GONE }
        val ink = dev.jraghavan.snmeeting.notes.QueryInkView(a) { png ->
            status.text = "Reading your handwriting…"; status.visibility = View.VISIBLE
            SearchJobs.query({ NativeBridge.nativeRecogniseQuery(png) }) { json ->
                if (a.isDestroyed) return@query
                val text = dev.jraghavan.snmeeting.notes.QueryStrip.text(json)
                if (text == null) { status.text = "Can't read handwriting offline — type the tag"; return@query }
                field.setText(text); field.setSelection(field.text.length)
                status.visibility = View.GONE
            }
        }
        val box = LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.dim(24), ui.dim(8), ui.dim(24), 0)
            addView(field, ui.fullWidth())
            addView(ui.eyebrow("Or write it", ui.inkSoft), ui.fullWidth().apply { topMargin = ui.dim(14) })
            addView(ink.apply { background = ui.outline(ui.dim(6)) }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ui.dim(120)).apply { topMargin = ui.dim(6) })
            addView(status, ui.fullWidth().apply { topMargin = ui.dim(6) })
        }
        AlertDialog.Builder(a, R.style.InkDialog).setTitle("Add a tag").setView(box)
            .setPositiveButton("ADD") { _, _ -> field.text.toString().trim().takeIf { it.isNotEmpty() }?.let(onTag) }
            .setNegativeButton("CANCEL", null)
            .show()
    }
}
