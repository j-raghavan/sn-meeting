package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.R
import dev.jraghavan.snmeeting.notes.ToolBarModel.Act
import dev.jraghavan.snmeeting.notes.ToolBarModel.Item
import dev.jraghavan.snmeeting.notes.canvas.ShapeStyle
import dev.jraghavan.snmeeting.notes.canvas.SizeStyle
import dev.jraghavan.snmeeting.notes.canvas.StylePalette

/**
 * DEVICE-UNVERIFIED (layout and refresh on the Manta). F17-FR2's docked tool bar, adapted from
 * inkread's `ToolPalette` (`Orientation.VERTICAL`, `Anchor`; AGPL-3.0, same owner) with Define
 * removed, dragging gone and a docked collapse: a vertical bar on the canvas's right (or left)
 * edge, never floating. It draws [ToolBarModel.items]; Mark and the timer redraw in place
 * ([showTimer], [showMarked], [showCost]) so a second's tick touches only its own view
 * (F17-FR11). F21-FR2: Shapes ▸, Insert ▸, Style ● and ⋮ open their panels beside the bar
 * (the host places them); with a selection the middle holds the selection's actions.
 */
class DockedToolBar(private val a: Activity, private val ui: Inkwell, private val host: Host) {
    interface Host {
        fun onTool(t: NoteTool)
        fun onUndo()
        fun onRedo()
        fun onCollapse(collapsed: Boolean)
        /** F21-FR2: a flyout or panel button: [panel] opens (or closes) beside [anchor]. */
        fun onPanel(panel: ToolBarModel.Panel, anchor: View)
        /** F21-FR2: one of the selection section's actions. */
        fun onAction(act: Act, anchor: View)
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
    /** F21-FR2: the bar is on the canvas's left (flyout markers point right). */
    var onLeft = false
    /** Fixed text sizes on the bar follow the device scale too ([dev.jraghavan.snmeeting.DeviceScale]). */
    private val tf = dev.jraghavan.snmeeting.DeviceScale.factor(a.resources.displayMetrics.densityDpi)

    init {
        // A margin under the bottom control (Play, Stop), so it never sits on the screen's edge.
        view.setPadding(0, 0, 0, ui.dp(BOTTOM_DP))
    }

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
        is Item.Shapes -> "shapes"
        is Item.Insert -> "insert"
        is Item.Style -> "style"
        is Item.More -> "more"
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
    } else if (isSquare(i)) {
        // A square button; the whole app is sized per device (DeviceScale).
        LinearLayout.LayoutParams(ui.dp(BUTTON_DP), ui.dp(BUTTON_DP)).apply {
            topMargin = ui.dp(GAP_DP); bottomMargin = ui.dp(GAP_DP)
        }
    } else {
        LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply {
            marginStart = ui.dp(4); marginEnd = ui.dp(4)
            // F21-AC17: every tool, the actions and the bottom group fit at 64 dp without scrolling.
            topMargin = ui.dp(GAP_DP); bottomMargin = ui.dp(GAP_DP)
        }
    }

    /** The icon buttons and the Style swatch: square, all one size. */
    private fun isSquare(i: Item) = when (i) {
        is Item.Tool, is Item.Shapes, is Item.Insert, is Item.Style, is Item.More, is Item.Undo, is Item.Redo, Item.Help -> true
        is Item.Action -> i.act != Act.FRONT && i.act != Act.BACK
        else -> false
    }

    /** A text control: centred, wrapping, and never under the 48 dp touch minimum if tappable. */
    private fun TextView.control(): TextView = apply {
        gravity = Gravity.CENTER
        if (isClickable) minHeight = ui.dp(TOUCH_DP)
    }

    private fun icon(res: Int, selected: Boolean, enabled: Boolean, open: Boolean = false, onClick: () -> Unit): ImageView = ImageView(a).apply {
        setImageResource(res)
        val box = ui.dp(BUTTON_DP)
        layoutParams = LinearLayout.LayoutParams(box, box)
        val pad = ui.dp(if (res.isCanvasIcon()) 11 else 8)
        setPadding(pad, pad, pad, pad)
        // The glyph keeps its own size in the square, shrinking only if the button is smaller.
        scaleType = ImageView.ScaleType.CENTER_INSIDE
        // inkread's palette: the active tool is a filled dark chip, the others plain line icons;
        // a button whose panel is open is a light chip.
        background = ui.outline(ui.dp(9), if (selected) ui.ink else if (open) OPEN_FILL else Color.WHITE)
        setColorFilter(if (selected) Color.WHITE else if (enabled) ui.ink else ui.textMuted)
        isEnabled = enabled
        isClickable = enabled
        contentDescription = resources.getResourceEntryName(res)
        if (enabled) setOnClickListener { onClick() }
    }

    private fun Int.isCanvasIcon() = a.resources.getResourceEntryName(this).startsWith("ic_cv_")

    /** A flyout button: its icon with a small ▸ (◂ docked right) pointing where it opens. */
    private fun flyout(res: Int, selected: Boolean, open: Boolean, onClick: (View) -> Unit): View {
        val frame = FrameLayout(a)
        val img = icon(res, selected, enabled = true, open = open) { onClick(frame) }
        frame.addView(img, FrameLayout.LayoutParams(ui.dp(BUTTON_DP), ui.dp(BUTTON_DP)))
        val mark = TextView(a).apply {
            text = if (onLeft) "▸" else "◂"
            textSize = 10f * tf
            setTextColor(if (selected) Color.WHITE else ui.ink)
        }
        frame.addView(mark, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.BOTTOM or if (onLeft) Gravity.END else Gravity.START).apply { setMargins(ui.dp(5), 0, ui.dp(5), ui.dp(2)) })
        return frame
    }

    /** F21-FR6: Style ●: the default colour as its e-ink grey, sized by the default size. */
    private inner class Swatch(private val style: ShapeStyle, private val open: Boolean) : View(a) {
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.style = Paint.Style.FILL }
        private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.style = Paint.Style.STROKE; strokeWidth = ui.dp(2).toFloat(); color = ui.ink }

        init {
            background = ui.outline(ui.dp(9), if (open) OPEN_FILL else Color.WHITE)
            minimumHeight = ui.dp(BUTTON_DP)
            contentDescription = "style"
        }

        /** One button square: a plain View would otherwise take every pixel the bar offers and
         *  push Undo, Redo, ⋮ and the bottom group off the screen. */
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) =
            setMeasuredDimension(getDefaultSize(ui.dp(BUTTON_DP), widthMeasureSpec), ui.dp(BUTTON_DP))

        override fun onDraw(canvas: Canvas) {
            val r = ui.dp(when (style.size) { SizeStyle.S -> 8; SizeStyle.M -> 11; SizeStyle.L -> 14; SizeStyle.XL -> 17 }).toFloat()
            fill.color = StylePalette.screen.stroke(style.color)
            canvas.drawCircle(width / 2f, height / 2f, r, fill)
            canvas.drawCircle(width / 2f, height / 2f, r, ring)
        }
    }

    private fun actIcon(act: Act) = when (act) {
        Act.DELETE -> R.drawable.ic_cv_action_delete
        Act.DUPLICATE -> R.drawable.ic_cv_action_duplicate
        Act.GROUP -> R.drawable.ic_cv_action_group
        Act.UNGROUP -> R.drawable.ic_cv_action_ungroup
        Act.LINK -> R.drawable.ic_cv_action_link
        Act.TABLE -> R.drawable.ic_cv_tool_table
        Act.DONE -> R.drawable.ic_cv_action_close
        Act.FRONT, Act.BACK, Act.STYLE -> 0
    }

    private fun action(i: Item.Action, s: ToolBarModel.State): View = when (i.act) {
        Act.FRONT, Act.BACK -> ui.eyebrow(if (i.act == Act.FRONT) "FRONT" else "BACK").apply {
            setPadding(0, ui.dp(6), 0, ui.dp(6))
            isClickable = true
            setOnClickListener { host.onAction(i.act, this) }
        }.control()
        Act.STYLE -> Swatch(s.style, s.open == ToolBarModel.Panel.STYLE).apply { setOnClickListener { host.onAction(i.act, this) } }
        else -> icon(actIcon(i.act), selected = false, enabled = i.enabled) {}.also { v ->
            if (i.enabled) v.setOnClickListener { host.onAction(i.act, v) }
        }
    }

    private fun iconOf(t: NoteTool) = when (t) {
        NoteTool.SELECT -> R.drawable.ic_cv_tool_select
        NoteTool.PEN -> R.drawable.ic_tool_pen
        NoteTool.HIGHLIGHTER -> R.drawable.ic_tool_highlighter
        NoteTool.ERASER -> R.drawable.ic_tool_eraser
        NoteTool.LASSO -> R.drawable.ic_tool_lasso
        NoteTool.LISTEN -> R.drawable.ic_tool_listen
        NoteTool.RECTANGLE -> R.drawable.ic_cv_tool_rectangle
        NoteTool.ELLIPSE -> R.drawable.ic_cv_tool_ellipse
        NoteTool.LINE -> R.drawable.ic_cv_tool_line
        NoteTool.ARROW -> R.drawable.ic_cv_action_arrow
        NoteTool.TEXT -> R.drawable.ic_cv_tool_text
        NoteTool.NOTE -> R.drawable.ic_cv_tool_note
        NoteTool.TABLE -> R.drawable.ic_cv_tool_table
    }

    fun iconFor(t: NoteTool) = iconOf(t)

    private fun build(i: Item, s: ToolBarModel.State): View = when (i) {
        is Item.Handle -> ui.eyebrow(if (i.collapsed) "‹ ›" else "› ‹").apply {
            setPadding(0, ui.dp(10), 0, ui.dp(4))
            isClickable = true
            setOnClickListener { host.onCollapse(!i.collapsed) }
        }.control()
        is Item.Tool -> icon(iconOf(i.tool), i.selected, i.enabled) {
            if (s.collapsed) host.onCollapse(false) else host.onTool(i.tool)
        }
        is Item.Shapes -> flyout(iconOf(i.shown), i.selected, i.open) { host.onPanel(ToolBarModel.Panel.SHAPES, it) }
        is Item.Insert -> flyout(R.drawable.ic_cv_tool_image, i.selected, i.open) { host.onPanel(ToolBarModel.Panel.INSERT, it) }
        is Item.Style -> Swatch(i.style, i.open).apply { setOnClickListener { host.onPanel(ToolBarModel.Panel.STYLE, this) } }
        is Item.More -> icon(R.drawable.ic_cv_action_more, selected = false, enabled = true, open = i.open) {}.also { v ->
            v.setOnClickListener { host.onPanel(ToolBarModel.Panel.MORE, v) }
        }
        is Item.Action -> action(i, s)
        is Item.Undo -> icon(R.drawable.ic_sel_undo, false, i.enabled) { host.onUndo() }
        is Item.Redo -> icon(R.drawable.ic_sel_redo, false, i.enabled) { host.onRedo() }
        // "0:12:41" on one line: a smaller mono with no letter spacing fits the button's width.
        is Item.Timer -> ui.eyebrow(i.text).apply { textSize = TIMER_SP * tf; letterSpacing = 0f; maxLines = 1; timer = this }.control()
        is Item.Cost -> ui.eyebrow(i.text.uppercase(), ui.inkSoft).apply { textSize = 8f * tf; letterSpacing = 0f; cost = this }.control()
        is Item.Mark -> ui.pill(stacked(i.label), size = PILL_SP) { host.onMark() }.apply { narrow(); mark = this }
        Item.Stop -> ui.pill(stacked("■  Stop"), filled = true, size = PILL_SP) { host.onStop() }.apply { narrow() }
        is Item.Play -> ui.pill(if (i.playing) "❚❚" else "▶", filled = i.playing, size = 16f) { if (i.enabled) host.onPlay() }.apply {
            narrow()
            if (!i.enabled) { setTextColor(ui.textMuted); isClickable = false }
        }
        // A round (?) the size of the other buttons, not a bare character.
        Item.Help -> TextView(a).apply {
            text = "?"
            gravity = Gravity.CENTER
            textSize = HELP_SP * tf
            typeface = android.graphics.Typeface.DEFAULT_BOLD
            setTextColor(ui.ink)
            background = android.graphics.drawable.GradientDrawable().apply {
                shape = android.graphics.drawable.GradientDrawable.OVAL
                setColor(Color.WHITE); setStroke(ui.dp(2), ui.ink)
            }
            contentDescription = "help"
            isClickable = true
            setOnClickListener { host.onHelp() }
        }
        is Item.Status -> ui.eyebrow(stacked(i.text).uppercase(), ui.inkSoft).apply { textSize = 8f * tf; letterSpacing = 0f }.control()
        Item.Gap -> View(a)
    }

    companion object {
        /** A square tool button, and the bar: the button plus 4 dp each side (was ~130 dp). */
        const val BUTTON_DP = 56
        const val BAR_DP = 64
        const val TOUCH_DP = 48
        const val TIMER_SP = 11f
        /** The space above and below each control (F21-AC17: the longer bar still fits). */
        const val GAP_DP = 3
        /** A button whose panel is open: a light chip (F21-FR2). */
        const val OPEN_FILL = 0xFFDDDDDD.toInt()
        const val PILL_SP = 13f
        const val HELP_SP = 22f
        const val BOTTOM_DP = 10

        /** "◆  Mark" → "◆\nMark", "Marked 0:12:41" → "Marked\n0:12:41": two short lines fit
         *  the narrow bar where one long one doesn't. */
        fun stacked(label: String): String = label.trim().replace(Regex("\\s+"), "\n")
    }
}
