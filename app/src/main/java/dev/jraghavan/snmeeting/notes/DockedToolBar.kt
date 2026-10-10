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
        /** F17-FR14: "?" toggles the hints. */
        fun onHelp()
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
    /** F17-FR14: each control's view, by the hint id that points at it. */
    val anchors = HashMap<String, View>()

    /** The bar's width, expanded or collapsed: one button and a small margin each side, in
     *  plain dp (not the page's width-driven scale), so the canvas gets the rest (owner,
     *  2026-10-09: the 130 dp bar was too wide). Collapsing folds the tools away, not the width. */
    val expandedPx get() = ui.dp(BAR_DP)
    val collapsedPx get() = ui.dp(BAR_DP)

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
        anchors.clear()
        timer = null; mark = null; cost = null
        items.forEach { i ->
            val v = build(i, s)
            hintId(i)?.let { anchors[it] = v }
            view.addView(v, params(i))
        }
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
    fun showMarked(label: String) { mark?.takeIf { it.text != stacked(label) }?.let { ui.stylePill(it, stacked(label), filled = false) } }

    /** A pill one button wide: its label stacked ("◆" over "Mark"), the touch minimum kept. */
    private fun TextView.narrow() {
        setPadding(ui.dp(2), ui.dp(8), ui.dp(2), ui.dp(8))
        minHeight = ui.dp(TOUCH_DP)
    }

    private fun hintId(i: Item): String? = when (i) {
        is Item.Tool -> i.tool.name.lowercase()
        is Item.Undo -> "undo"
        is Item.Handle -> "handle"
        is Item.Mark -> "mark"
        Item.Stop -> "stop"
        Item.Help -> "help"
        else -> null
    }

    /** Every control is one button wide; a label too long for it wraps onto a second line. */
    private fun params(i: Item) = if (i == Item.Gap) {
        LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f)
    } else {
        LinearLayout.LayoutParams(ui.dp(BUTTON_DP), LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            topMargin = ui.dp(5); bottomMargin = ui.dp(5)
        }
    }

    /** A text control: centred, wrapping, and never under the 48 dp touch minimum if tappable. */
    private fun TextView.control(): TextView = apply {
        gravity = Gravity.CENTER
        if (isClickable) minHeight = ui.dp(TOUCH_DP)
    }

    private fun icon(res: Int, selected: Boolean, enabled: Boolean, onClick: () -> Unit): View = ImageView(a).apply {
        setImageResource(res)
        val box = ui.dp(BUTTON_DP)
        layoutParams = LinearLayout.LayoutParams(box, box)
        val pad = ui.dp(8) // the glyph stays ~40 dp, as before
        setPadding(pad, pad, pad, pad)
        // inkread's palette: the active tool is a filled dark chip, the others plain line icons.
        background = ui.outline(ui.dp(9), if (selected) ui.ink else Color.WHITE)
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
            setPadding(0, ui.dp(10), 0, ui.dp(4))
            isClickable = true
            setOnClickListener { host.onCollapse(!i.collapsed) }
        }.control()
        is Item.Tool -> icon(iconOf(i.tool), i.selected, i.enabled) {
            if (s.collapsed) host.onCollapse(false) else host.onTool(i.tool)
        }
        is Item.Undo -> icon(R.drawable.ic_sel_undo, false, i.enabled) { host.onUndo() }
        is Item.Redo -> icon(R.drawable.ic_sel_redo, false, i.enabled) { host.onRedo() }
        is Item.Options -> ui.eyebrow(if (i.focus) "FOCUS ✓" else "· · ·").apply {
            setPadding(0, ui.dp(6), 0, ui.dp(6))
            isClickable = true
            setOnClickListener { host.onOptions(this) }
        }.control()
        // "0:12:41" on one line: a smaller mono with no letter spacing fits the button's width.
        is Item.Timer -> ui.eyebrow(i.text).apply { textSize = TIMER_SP; letterSpacing = 0f; maxLines = 1; timer = this }.control()
        is Item.Cost -> ui.eyebrow(i.text.uppercase(), ui.inkSoft).apply { textSize = 8f; letterSpacing = 0f; cost = this }.control()
        is Item.Mark -> ui.pill(stacked(i.label), size = PILL_SP) { host.onMark() }.apply { narrow(); mark = this }
        Item.Stop -> ui.pill(stacked("■  Stop"), filled = true, size = PILL_SP) { host.onStop() }.apply { narrow() }
        is Item.Play -> ui.pill(if (i.playing) "❚❚" else "▶", filled = i.playing, size = 16f) { if (i.enabled) host.onPlay() }.apply {
            narrow()
            if (!i.enabled) { setTextColor(ui.textMuted); isClickable = false }
        }
        Item.Help -> ui.eyebrow("?").apply {
            setPadding(0, ui.dp(6), 0, ui.dp(6))
            isClickable = true
            setOnClickListener { host.onHelp() }
        }.control()
        Item.Gap -> View(a)
    }

    companion object {
        /** A square tool button, and the bar: the button plus 4 dp each side (was ~130 dp). */
        const val BUTTON_DP = 56
        const val BAR_DP = 64
        const val TOUCH_DP = 48
        const val TIMER_SP = 11f
        const val PILL_SP = 13f

        /** "◆  Mark" → "◆\nMark", "Marked 0:12:41" → "Marked\n0:12:41": two short lines fit
         *  the narrow bar where one long one doesn't. */
        fun stacked(label: String): String = label.trim().replace(Regex("\\s+"), "\n")
    }
}
