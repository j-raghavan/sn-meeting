package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.R
import dev.jraghavan.snmeeting.notes.ToolBarModel.Item

/**
 * DEVICE-UNVERIFIED (layout and refresh on the Manta). F17-FR2's docked tool bar, adapted from
 * inkread's `ToolPalette` (`Orientation.VERTICAL`, `Anchor`; AGPL-3.0, same owner) with Define
 * removed, dragging gone and a docked collapse: a vertical bar on the canvas's right (or left)
 * edge, never floating. It draws [ToolBarModel.items]; Mark and the timer redraw in place
 * ([showTimer], [showMarked], [showCost]) so a second's tick touches only its own view
 * (F17-FR11).
 */
class DockedToolBar(private val a: Activity, private val ui: Inkwell, private val host: Host) {
    interface Host {
        fun onTool(t: NoteTool)
        fun onUndo()
        fun onRedo()
        fun onCollapse(collapsed: Boolean)
        fun onOptions(anchor: View)
        fun onMark()
        fun onStop()
        fun onPlay()
    }

    val view: LinearLayout = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        setBackgroundColor(Color.WHITE)
    }
    private var timer: TextView? = null
    private var mark: TextView? = null
    private var cost: TextView? = null
    private var shown: List<Item> = emptyList()

    /** Widths of the bar, expanded and collapsed (the canvas starts beside it). */
    val expandedPx get() = ui.dim(112)
    val collapsedPx get() = ui.dim(76)

    /** Rebuild only when the items differ (a tool change, collapse, Undo becoming available). */
    fun render(s: ToolBarModel.State) {
        val items = ToolBarModel.items(s)
        if (items.map(::key) == shown.map(::key)) {
            items.forEach { update(it) }
            shown = items
            return
        }
        shown = items
        view.removeAllViews()
        timer = null; mark = null; cost = null
        items.forEach { view.addView(build(it, s), params(it)) }
    }

    /** Two items with the same key are drawn by the same view: their text may change in place. */
    private fun key(i: Item): Any = when (i) {
        is Item.Timer -> "timer"
        is Item.Cost -> "cost"
        is Item.Mark -> "mark"
        else -> i
    }

    private fun update(i: Item) {
        when (i) {
            is Item.Timer -> showTimer(i.text)
            is Item.Cost -> showCost(i.text)
            is Item.Mark -> showMarked(i.label)
            else -> Unit
        }
    }

    fun showTimer(text: String) { timer?.takeIf { it.text != text }?.text = text }
    fun showCost(text: String) { cost?.takeIf { it.text != text.uppercase() }?.text = text.uppercase() }
    fun showMarked(label: String) { mark?.takeIf { it.text != label }?.let { ui.stylePill(it, label, filled = false) } }

    private fun params(i: Item) = if (i == Item.Gap) {
        LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f)
    } else {
        LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            topMargin = ui.dim(8); bottomMargin = ui.dim(8)
        }
    }

    private fun icon(res: Int, selected: Boolean, enabled: Boolean, onClick: () -> Unit): View = ImageView(a).apply {
        setImageResource(res)
        val box = ui.dim(64)
        layoutParams = LinearLayout.LayoutParams(box, box)
        val pad = ui.dim(14)
        setPadding(pad, pad, pad, pad)
        // inkread's palette: the active tool is a filled dark chip, the others plain line icons.
        background = ui.outline(ui.dim(10), if (selected) ui.ink else Color.WHITE)
        setColorFilter(if (selected) Color.WHITE else if (enabled) ui.ink else ui.textMuted)
        isEnabled = enabled
        isClickable = enabled
        if (enabled) setOnClickListener { onClick() }
    }

    private fun iconOf(t: NoteTool) = when (t) {
        NoteTool.PEN -> R.drawable.ic_tool_pen
        NoteTool.HIGHLIGHTER -> R.drawable.ic_tool_highlighter
        NoteTool.ERASER -> R.drawable.ic_tool_eraser
        NoteTool.LASSO -> R.drawable.ic_tool_lasso
        NoteTool.LISTEN -> R.drawable.ic_tool_listen
    }

    private fun build(i: Item, s: ToolBarModel.State): View = when (i) {
        is Item.Handle -> ui.eyebrow(if (i.collapsed) "‹ ›" else "› ‹").apply {
            gravity = Gravity.CENTER
            setPadding(ui.dim(12), ui.dim(14), ui.dim(12), ui.dim(6))
            isClickable = true
            setOnClickListener { host.onCollapse(!i.collapsed) }
        }
        is Item.Tool -> icon(iconOf(i.tool), i.selected, i.enabled) {
            if (s.collapsed) host.onCollapse(false) else host.onTool(i.tool)
        }
        is Item.Undo -> icon(R.drawable.ic_sel_undo, false, i.enabled) { host.onUndo() }
        is Item.Redo -> icon(R.drawable.ic_sel_redo, false, i.enabled) { host.onRedo() }
        is Item.Options -> ui.eyebrow(if (i.focus) "FOCUS ✓" else "· · ·").apply {
            gravity = Gravity.CENTER
            setPadding(ui.dim(8), ui.dim(10), ui.dim(8), ui.dim(10))
            isClickable = true
            setOnClickListener { host.onOptions(this) }
        }
        is Item.Timer -> ui.eyebrow(i.text).apply { gravity = Gravity.CENTER; timer = this }
        is Item.Cost -> ui.eyebrow(i.text.uppercase(), ui.inkSoft).apply { gravity = Gravity.CENTER; textSize = ui.fs(9f); cost = this }
        is Item.Mark -> ui.pill(i.label, size = 14f) { host.onMark() }.apply {
            setPadding(ui.dim(10), ui.dim(12), ui.dim(10), ui.dim(12)); mark = this
        }
        Item.Stop -> ui.pill("■  Stop", filled = true, size = 14f) { host.onStop() }.apply {
            setPadding(ui.dim(10), ui.dim(12), ui.dim(10), ui.dim(12))
        }
        is Item.Play -> ui.pill(if (i.playing) "❚❚" else "▶", filled = i.playing, size = 16f) { if (i.enabled) host.onPlay() }.apply {
            if (!i.enabled) { setTextColor(ui.textMuted); isClickable = false }
        }
        Item.Gap -> View(a)
    }
}
