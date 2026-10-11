// The panel's content and behaviour follow sn-canvas `src/ui/StylePanel.tsx` (MIT License,
// Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md), re-implemented as Inkwell
// views; its fill and outline icons are sn-canvas's `assets/icons/fill-*.png`, `dash-*.png`.
package dev.jraghavan.snmeeting.notes.canvas

import android.app.Activity
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.view.Gravity
import android.view.View
import android.widget.GridLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.R

/**
 * F21-FR6: which options apply where (the core's `style::applies`, kept in step): fill to
 * rectangles, ellipses and sticky notes; outline to shapes and images (none to images alone);
 * on strokes colour, opacity and size; on text colour and size. With nothing selected every
 * option applies (it sets the default).
 */
object StyleRules {
    fun applies(kind: String, property: String, value: String): Boolean = when (property) {
        "color" -> true
        "opacity" -> kind != "text"
        "size" -> kind != "image"
        "fill" -> kind in setOf("rectangle", "ellipse", "note")
        "dash" -> when (kind) {
            "image" -> true
            "rectangle", "ellipse", "line", "arrow" -> value != "none"
            else -> false
        }
        else -> false
    }

    /** Whether the option is live for a selection of [kinds] (`stroke` for ink); dimmed if not. */
    fun enabled(kinds: Set<String>, property: String, value: String): Boolean =
        if (kinds.isEmpty()) !(property == "dash" && value == "none") else kinds.any { applies(it, property, value) }
}

/**
 * DEVICE-UNVERIFIED (look on the Manta). F21-FR6's style panel, beside the bar: the 12 colours
 * as their e-ink greys with the picked colour's name under the grid, opacity, fill, outline
 * and size. A pick applies to the selection as one step and becomes the default; options that
 * don't apply to the selection are dimmed.
 */
class StylePanel(private val a: Activity, private val ui: Inkwell) {
    fun build(style: ShapeStyle, kinds: Set<String>, onPick: (String, String) -> Unit): View = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(ui.dp(6), ui.dp(4), ui.dp(6), ui.dp(4))
        addView(label("COLOUR"))
        val grid = GridLayout(a).apply { columnCount = 4 }
        for (c in StyleColor.entries) {
            grid.addView(Dot(c, c == style.color).apply { setOnClickListener { onPick("color", c.id) } },
                GridLayout.LayoutParams().apply { width = ui.dp(ROW_DP / 4); height = ui.dp(ROW_DP / 4) })
        }
        addView(grid)
        addView(ui.note(style.color.label, 13f).apply { gravity = Gravity.CENTER })
        val opacityOn = StyleRules.enabled(kinds, "opacity", "1")
        addView(label("OPACITY  ${(style.opacity * 100).toInt()} %"))
        addView(SeekBar(a).apply {
            max = 9
            progress = ((style.opacity * 10).toInt() - 1).coerceIn(0, 9)
            isEnabled = opacityOn
            alpha = if (opacityOn) 1f else DIM
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(s: SeekBar, p: Int, user: Boolean) {}
                override fun onStartTrackingTouch(s: SeekBar) {}
                // One pick when the pen lifts, not one per step (no live redraw on e-ink).
                override fun onStopTrackingTouch(s: SeekBar) = onPick("opacity", ((s.progress + 1) / 10.0).toString())
            })
        }, LinearLayout.LayoutParams(ui.dp(ROW_DP), ui.dp(40)))
        addView(label("FILL"))
        addView(icons(FillStyle.entries.map { Triple(fillIcon(it), it.id, it == style.fill) }, kinds, "fill", onPick), rowParams())
        addView(label("OUTLINE"))
        addView(icons(DashStyle.entries.map { Triple(dashIcon(it), it.id, it == style.dash) }, kinds, "dash", onPick), rowParams())
        addView(label("SIZE"))
        addView(LinearLayout(a).apply {
            SizeStyle.entries.forEach { sz ->
                val on = StyleRules.enabled(kinds, "size", sz.id)
                addView(ui.chip(sz.id.uppercase(), filled = sz == style.size).apply {
                    gravity = Gravity.CENTER
                    alpha = if (on) 1f else DIM
                    isEnabled = on
                    if (on) setOnClickListener { onPick("size", sz.id) }
                }, cell(SizeStyle.entries.size, ui.dp(44)))
            }
        }, rowParams())
    }

    /** Every row is as wide as the colour grid, so the panel never grows past it. */
    private fun rowParams() = LinearLayout.LayoutParams(ui.dp(ROW_DP), LinearLayout.LayoutParams.WRAP_CONTENT)

    /** One of [n] equal cells sharing the row (any number of options fits), at most [max] tall. */
    private fun cell(n: Int, max: Int) = LinearLayout.LayoutParams(0, minOf(max, (ui.dp(ROW_DP) - ui.dp(GAP_DP) * (n - 1)) / n), 1f)
        .apply { marginEnd = ui.dp(GAP_DP) }

    private fun label(t: String) = ui.eyebrow(t).apply { setPadding(0, ui.dp(8), 0, ui.dp(4)) }

    private fun icons(entries: List<Triple<Int, String, Boolean>>, kinds: Set<String>, property: String, onPick: (String, String) -> Unit) =
        LinearLayout(a).apply {
            entries.forEach { (res, id, selected) ->
                val on = StyleRules.enabled(kinds, property, id)
                addView(ImageView(a).apply {
                    setImageResource(res)
                    val pad = ui.dp(6)
                    setPadding(pad, pad, pad, pad)
                    background = ui.outline(ui.dp(8), if (selected) ui.ink else Color.WHITE)
                    setColorFilter(if (selected) Color.WHITE else ui.ink)
                    alpha = if (on) 1f else DIM
                    isEnabled = on
                    contentDescription = "$property $id"
                    if (on) setOnClickListener { onPick(property, id) }
                }, cell(entries.size, ui.dp(40)))
            }
        }

    private fun fillIcon(f: FillStyle) = when (f) {
        FillStyle.NONE -> R.drawable.ic_cv_fill_none
        FillStyle.SEMI -> R.drawable.ic_cv_fill_semi
        FillStyle.SOLID -> R.drawable.ic_cv_fill_solid
        FillStyle.PATTERN -> R.drawable.ic_cv_fill_pattern
        FillStyle.GRADIENT -> R.drawable.ic_cv_fill_gradient
    }

    private fun dashIcon(d: DashStyle) = when (d) {
        DashStyle.DRAW -> R.drawable.ic_cv_dash_draw
        DashStyle.DASHED -> R.drawable.ic_cv_dash_dashed
        DashStyle.DOTTED -> R.drawable.ic_cv_dash_dotted
        DashStyle.SOLID -> R.drawable.ic_cv_dash_solid
        DashStyle.NONE -> R.drawable.ic_cv_dash_none
    }

    /** One swatch: the colour as its e-ink grey; the picked one ringed twice. */
    private inner class Dot(private val c: StyleColor, private val picked: Boolean) : View(a) {
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL; color = StylePalette.screen.stroke(c) }
        private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; color = Color.BLACK }

        init { contentDescription = c.label }

        override fun onDraw(canvas: Canvas) {
            val r = minOf(width, height) / 2f - ui.dp(8)
            canvas.drawCircle(width / 2f, height / 2f, r, fill)
            ring.strokeWidth = ui.dp(if (picked) 3 else 1).toFloat()
            canvas.drawCircle(width / 2f, height / 2f, if (picked) r + ui.dp(4) else r, ring)
        }
    }

    private companion object {
        const val DIM = 0.3f
        /** The panel's content width: the colour grid's four 48 dp columns. */
        const val ROW_DP = 4 * 48
        const val GAP_DP = 4
    }
}
