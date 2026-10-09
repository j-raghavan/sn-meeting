package dev.jraghavan.snmeeting

import android.app.Activity
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.HomeRows

/**
 * Home's two side-by-side lists (F6-FR1, owner decision 2026-10-07): the recent meetings on the
 * left and every open to-do on the right, equal-height cards that scroll inside, each heading
 * with a count. Rows are rebuilt only when the core's answer arrives.
 */
class HomeLists(private val a: Activity, private val ui: Inkwell, private val onMeeting: (String) -> Unit) {
    private val meetingTitle = ui.eyebrowItalic("Recent meetings")
    private val todoTitle = ui.eyebrowItalic("Open to-dos")
    private val meetingRows = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
    private val todoRows = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }

    /** Two equal columns, equal heights, with a gutter. */
    val view: View = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        addView(section(meetingTitle, meetingRows), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f).apply { marginEnd = ui.dim(12) })
        addView(section(todoTitle, todoRows), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f).apply { marginStart = ui.dim(12) })
    }

    /** Heading over a card that fills the column's height and scrolls inside. */
    private fun section(title: TextView, rows: LinearLayout): View = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        addView(title, ui.fullWidth())
        addView(ui.spacer(ui.dim(14)))
        addView(ui.scrollCard(rows), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    private fun empty(rows: LinearLayout, text: String) {
        rows.addView(ui.note(text).apply { setPadding(0, ui.dim(14), 0, ui.dim(14)) })
    }

    /** [error]: the core couldn't list them; shown in place of the rows. */
    fun showMeetings(meetings: List<HomeRows.Meeting>, error: String? = null) {
        meetingRows.removeAllViews()
        meetingTitle.text = if (error != null) "Recent meetings" else "Recent meetings  ·  ${meetings.size}"
        if (error != null) return empty(meetingRows, "Couldn't read the meetings: $error")
        if (meetings.isEmpty()) return empty(meetingRows, "No meetings yet. Tap ＋ New to record one.")
        meetings.forEachIndexed { i, m ->
            if (i > 0) meetingRows.addView(ui.hairline())
            meetingRows.addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                isClickable = true
                setOnClickListener { onMeeting(m.id) }
                addView(LinearLayout(a).apply {
                    orientation = LinearLayout.VERTICAL
                    // Title first (serif, bold); an untitled meeting reads muted until it is named.
                    addView(ui.body(m.title, 18f, bold = !m.untitled).apply {
                        if (m.untitled) { setTextColor(ui.textMuted); typeface = ui.serifItalic }
                    })
                    addView(ui.eyebrow(m.stamp), ui.wrap().apply { topMargin = ui.dim(4) })
                    addView(ui.note(m.detail), ui.wrap().apply { topMargin = ui.dim(3) })
                    // Half-width column: the status chip sits under the details, not beside them.
                    addView(ui.chip(m.chip, filled = m.attention), ui.wrap().apply { topMargin = ui.dim(8) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.body("→", 18f).apply { setPadding(ui.dim(16), 0, 0, 0) })
            })
        }
    }

    /**
     * S1: no all-files access yet. In place of the meetings, why it is needed and a Grant access
     * pill that opens the system screen (only when tapped).
     */
    fun showStorageNeeded(text: String, onGrant: () -> Unit) {
        meetingRows.removeAllViews(); todoRows.removeAllViews()
        meetingTitle.text = "Recent meetings"; todoTitle.text = "Open to-dos"
        meetingRows.addView(ui.eyebrow("All-files access"), ui.wrap().apply { topMargin = ui.dim(14) })
        meetingRows.addView(ui.body(text, 17f), ui.wrap().apply { topMargin = ui.dim(8) })
        meetingRows.addView(ui.pill("Grant access", size = 16f, onClick = onGrant), ui.wrap().apply { topMargin = ui.dim(16); bottomMargin = ui.dim(14) })
        empty(todoRows, "To-dos appear here once Meetings can read your recordings.")
    }

    /** Open to-dos from every meeting, not just the five listed. [onTick] marks one done. */
    fun showTodos(todos: List<HomeRows.Todo>, error: String? = null, onTick: ((HomeRows.Todo) -> Unit)? = null) {
        todoRows.removeAllViews()
        todoTitle.text = if (error != null) "Open to-dos" else "Open to-dos  ·  ${todos.size}"
        if (error != null) return empty(todoRows, "Couldn't read the to-dos: $error")
        if (todos.isEmpty()) return empty(todoRows, "Nothing open. To-dos appear here after a meeting is transcribed.")
        todos.forEachIndexed { i, t ->
            if (i > 0) todoRows.addView(ui.hairline())
            todoRows.addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                addView(ui.checkbox(false).apply {
                    if (onTick != null) {
                        isClickable = true
                        setOnClickListener {
                            // Ticked at once (one redraw); the row leaves when the list is re-read.
                            isClickable = false; text = "✓"; setTextColor(android.graphics.Color.WHITE)
                            background = ui.outline(ui.dim(3), ui.ink)
                            onTick(t)
                        }
                    }
                })
                addView(LinearLayout(a).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(ui.dim(16), 0, 0, 0)
                    addView(ui.body(t.text, 17f))
                    if (t.meta.isNotEmpty()) addView(ui.note(t.meta, 13f), ui.wrap().apply { topMargin = ui.dim(3) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
        }
    }
}
