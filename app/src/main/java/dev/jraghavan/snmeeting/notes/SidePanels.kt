package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.Inkwell

/**
 * F21-FR2: what ⋮ offers, top to bottom, for the page's state. Pure, so the rules are
 * JVM-tested. Entries arrive with their FRs: front and back with a selection (FR5), Table ▸
 * with a target cell (FR10), Clear canvas when the canvas holds something (FR3), Restore
 * cleared canvas while a backup exists (OQ34), New canvas and Canvases… (FR12), Export to PDF
 * (FR14), then the view's own: zoom, Focus and a full refresh (moved here from Options).
 */
object MoreMenu {
    enum class Entry(val label: String) {
        FRONT("Bring to front"),
        BACK("Send to back"),
        ZOOM_FIT("Zoom to fit"),
        ZOOM_100("Zoom to 100 %"),
        TABLE("Table ▸"),
        CLEAR("Clear canvas"),
        RESTORE("Restore cleared canvas"),
        NEW_CANVAS("New canvas"),
        CANVASES("Canvases…"),
        EXPORT("Export to PDF"),
        FOCUS_ON("Focus on"),
        FOCUS_OFF("Focus off"),
        REFRESH("Refresh the screen"),
    }

    data class State(
        val selection: Boolean = false,
        val tableCell: Boolean = false,
        val hasContent: Boolean = false,
        val canRestore: Boolean = false,
        val focus: Boolean = false,
        val canvases: Boolean = false,
        val export: Boolean = false,
    )

    fun items(s: State): List<Entry> = listOfNotNull(
        Entry.FRONT.takeIf { s.selection },
        Entry.BACK.takeIf { s.selection },
        Entry.ZOOM_FIT,
        Entry.ZOOM_100,
        Entry.TABLE.takeIf { s.tableCell },
        Entry.CLEAR.takeIf { s.hasContent },
        Entry.RESTORE.takeIf { s.canRestore },
        Entry.NEW_CANVAS.takeIf { s.canvases },
        Entry.CANVASES.takeIf { s.canvases },
        Entry.EXPORT.takeIf { s.export },
        if (s.focus) Entry.FOCUS_OFF else Entry.FOCUS_ON,
        Entry.REFRESH,
    )
}

/**
 * DEVICE-UNVERIFIED (placement and refresh on the Manta). F21-FR2: one panel at a time beside
 * the docked bar, level with the button that opened it, on the canvas's near edge: Shapes ▸,
 * Insert ▸, the style panel, ⋮ and the Eraser's Clear canvas. Nothing floats over the canvas's
 * far side. While one is open the page turns firmware ink off over the window, so a pen tap on
 * it never inks; closing it bakes once (the [onChange] host does both).
 */
class SidePanels(
    private val a: Activity,
    private val ui: Inkwell,
    private val layer: FrameLayout,
    private val barOnLeft: () -> Boolean,
    private val barWidthPx: () -> Int,
    private val onChange: (open: View?) -> Unit,
) {
    var open: ToolBarModel.Panel? = null
        private set
    var view: View? = null
        private set

    /** Show [content] as [panel] beside the bar, level with [anchor] (kept on screen). */
    fun show(panel: ToolBarModel.Panel, anchor: View, content: View) {
        // A button the bar has just rebuilt isn't laid out yet: place the panel once it is.
        if (anchor.isAttachedToWindow && !anchor.isLaidOut) {
            anchor.viewTreeObserver.addOnGlobalLayoutListener(object : android.view.ViewTreeObserver.OnGlobalLayoutListener {
                override fun onGlobalLayout() {
                    anchor.viewTreeObserver.removeOnGlobalLayoutListener(this)
                    show(panel, anchor, content)
                }
            })
            return
        }
        removeView()
        val frame = LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            background = ui.outline(ui.dp(10), Color.WHITE)
            setPadding(ui.dp(6), ui.dp(6), ui.dp(6), ui.dp(6))
            isClickable = true // a tap between its controls stays on the panel, not the canvas
            addView(content)
        }
        frame.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
        val at = IntArray(2).also { anchor.getLocationInWindow(it) }
        val base = IntArray(2).also { layer.getLocationInWindow(it) }
        val room = (layer.height - frame.measuredHeight).coerceAtLeast(0)
        val top = (at[1] - base[1]).coerceIn(0, room)
        val left = barOnLeft()
        val side = barWidthPx() + ui.dp(4)
        val lp = FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.TOP or if (left) Gravity.START else Gravity.END).apply {
            topMargin = top
            if (left) marginStart = side else marginEnd = side
        }
        layer.addView(frame, lp)
        view = frame
        open = panel
        onChange(frame)
    }

    /** Close the open panel, if any; the page bakes once. */
    fun close() {
        if (view == null) return
        removeView()
        onChange(null)
    }

    private fun removeView() {
        view?.let { layer.removeView(it) }
        view = null
        open = null
    }

    /** A column of square icon buttons (a flyout). */
    fun icons(entries: List<Triple<Int, String, Boolean>>, onPick: (Int) -> Unit): View = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        entries.forEachIndexed { i, (res, label, selected) ->
            addView(ImageView(a).apply {
                setImageResource(res)
                val pad = ui.dp(11)
                setPadding(pad, pad, pad, pad)
                background = ui.outline(ui.dp(9), if (selected) ui.ink else Color.WHITE)
                setColorFilter(if (selected) Color.WHITE else ui.ink)
                contentDescription = label
                setOnClickListener { onPick(i) }
            }, LinearLayout.LayoutParams(ui.dp(DockedToolBar.BUTTON_DP), ui.dp(DockedToolBar.BUTTON_DP)).apply {
                topMargin = ui.dp(2); bottomMargin = ui.dp(2)
            })
        }
    }

    /** A column of text rows (⋮, Clear canvas), each at least the touch minimum tall. */
    fun rows(labels: List<String>, enabled: (Int) -> Boolean = { true }, onPick: (Int) -> Unit): View = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        labels.forEachIndexed { i, label ->
            val on = enabled(i)
            addView(TextView(a).apply {
                text = label
                textSize = 17f * dev.jraghavan.snmeeting.DeviceScale.factor(a.resources.displayMetrics.densityDpi)
                typeface = ui.serif
                setTextColor(if (on) ui.ink else ui.textMuted)
                isEnabled = on
                gravity = Gravity.CENTER_VERTICAL
                minHeight = ui.dp(DockedToolBar.TOUCH_DP)
                setPadding(ui.dp(14), 0, ui.dp(18), 0)
                if (on) setOnClickListener { onPick(i) }
            })
            if (i < labels.size - 1) addView(ui.hairline())
        }
        // As wide as the longest row: a full-width hairline would otherwise stretch the panel
        // across the whole canvas.
        val widest = (0 until childCount).map { getChildAt(it) }.filterIsInstance<TextView>().maxOfOrNull {
            it.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED); it.measuredWidth
        } ?: 0
        (0 until childCount).forEach { k -> getChildAt(k).layoutParams = LinearLayout.LayoutParams(widest, getChildAt(k).layoutParams?.height ?: LinearLayout.LayoutParams.WRAP_CONTENT) }
    }
}
