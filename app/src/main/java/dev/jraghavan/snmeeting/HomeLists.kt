package dev.jraghavan.snmeeting

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.AgendaDay
import dev.jraghavan.snmeeting.model.AgendaRow
import dev.jraghavan.snmeeting.model.CalendarFormat
import dev.jraghavan.snmeeting.model.Format
import dev.jraghavan.snmeeting.model.HomeRows
import dev.jraghavan.snmeeting.model.Marks
import dev.jraghavan.snmeeting.model.TodoRef
import java.time.LocalDate
import java.time.ZoneId

/**
 * Home's layout A (F18-FR7, owner mock-up 2026-10-09): the week strip, the day's timeline on the
 * left ("Today ‹ ›", events and recordings in time order, the current event boxed) and the open
 * to-dos on the right. Rows are rebuilt only when the core's answer for them changes; the
 * recording timer is the one view that ticks (NFR2).
 */
class HomeLists(private val a: Activity, private val ui: Inkwell, private val actions: Actions) {

    /** What a tap on Home does; MainActivity owns the flows. */
    interface Actions {
        fun onDay(d: LocalDate)
        fun onEvent(r: AgendaRow)
        fun onMeeting(id: String)
        fun onUnscheduled(r: AgendaRow)
        fun onRecordEvent(r: AgendaRow)
        fun onTick(t: TodoRef)
        fun onTodoSource(t: TodoRef)
        fun onRecent()
    }

    private val zone get() = ZoneId.systemDefault()

    val week: LinearLayout = LinearLayout(a).apply { orientation = LinearLayout.HORIZONTAL }
    private val dayTitle = ui.body("Today", 19f, bold = true)
    private val timeline = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
    private val todoTitle = ui.eyebrowItalic("Open to-dos").apply { gravity = Gravity.START }
    private val todoRows = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
    private val dayColumn = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }

    /** The recording timer inside the current event's box, when there is one (F18-FR7). */
    var timer: TextView? = null
        private set

    private fun arrow(text: String, onClick: () -> Unit) = ui.body(text, 22f).apply {
        setPadding(ui.dim(14), ui.dim(2), ui.dim(14), ui.dim(2)); isClickable = true; setOnClickListener { onClick() }
    }

    private var shown: LocalDate = LocalDate.now()
    /** Whether any calendar link is set: only then can a recording be linked (F18-FR10). */
    private var hasLinks = false

    /** The two columns: the day (wider) and the to-dos, equal heights, each scrolling inside. */
    val columns: View = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        dayColumn.addView(LinearLayout(a).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(dayTitle)
            addView(arrow("‹") { actions.onDay(shown.minusDays(1)) })
            addView(arrow("›") { actions.onDay(shown.plusDays(1)) })
            addView(View(a), LinearLayout.LayoutParams(0, 1, 1f))
            // OQ22: the five most recent meetings stay one tap away.
            addView(ui.eyebrow("Recent  ›").apply {
                setPadding(ui.dim(8), ui.dim(8), 0, ui.dim(8)); isClickable = true; setOnClickListener { actions.onRecent() }
            })
        }, ui.fullWidth())
        dayColumn.addView(ui.spacer(ui.dim(10)))
        dayColumn.addView(ui.scrollCard(timeline), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        addView(dayColumn, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 3f).apply { marginEnd = ui.dim(12) })
        addView(LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            addView(todoTitle, ui.fullWidth().apply { topMargin = ui.dim(6) })
            addView(ui.spacer(ui.dim(14)))
            addView(ui.scrollCard(todoRows), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 2f).apply { marginStart = ui.dim(12) })
    }

    /** Mon–Sun: today a filled circle, the shown day outlined, a dot under days with meetings. */
    fun showWeek(days: List<LocalDate>, today: LocalDate, selected: LocalDate, dots: Set<LocalDate>) {
        week.removeAllViews()
        days.forEach { d ->
            week.addView(LinearLayout(a).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_HORIZONTAL
                isClickable = true
                setOnClickListener { actions.onDay(d) }
                addView(ui.eyebrow(CalendarFormat.weekday(d), if (d == today) ui.ink else ui.textMuted).apply { gravity = Gravity.CENTER })
                val size = ui.dim(40)
                addView(TextView(a).apply {
                    text = d.dayOfMonth.toString(); gravity = Gravity.CENTER; textSize = ui.fs(17f); typeface = ui.serifBold
                    setTextColor(if (d == today) Color.WHITE else ui.ink)
                    background = GradientDrawable().apply {
                        shape = GradientDrawable.OVAL
                        setColor(if (d == today) ui.ink else Color.WHITE)
                        if (d == selected && d != today) setStroke(maxOf(1, ui.dp(2)), ui.ink)
                    }
                }, LinearLayout.LayoutParams(size, size).apply { topMargin = ui.dim(6) })
                val dot = ui.dim(7)
                addView(View(a).apply {
                    background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(ui.ink) }
                    visibility = if (d in dots) View.VISIBLE else View.INVISIBLE
                }, LinearLayout.LayoutParams(dot, dot).apply { topMargin = ui.dim(6) })
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
    }

    private fun empty(rows: LinearLayout, text: String) {
        rows.addView(ui.note(text).apply { setPadding(0, ui.dim(14), 0, ui.dim(14)) })
    }

    /** The day's rows. [recording]: a meeting is being recorded (the box shows its timer). */
    fun showDay(day: AgendaDay?, d: LocalDate, today: LocalDate, hasLinks: Boolean, recording: Boolean, error: String? = null) {
        shown = d
        this.hasLinks = hasLinks
        dayTitle.text = CalendarFormat.dayTitle(d, today)
        timeline.removeAllViews()
        timer = null
        if (error != null) return empty(timeline, "Couldn't read the day: $error")
        val all = day?.allDay.orEmpty()
        val rows = day?.rows.orEmpty()
        if (all.isEmpty() && rows.isEmpty()) {
            return empty(timeline, "Nothing on this day. Tap ＋ New to record a meeting.")
        }
        all.forEach { r ->
            timeline.addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(10), 0, ui.dim(10))
                isClickable = true
                setOnClickListener { actions.onEvent(r) }
                addView(ui.eyebrow("All day", ui.textMuted), LinearLayout.LayoutParams(ui.dim(110), LinearLayout.LayoutParams.WRAP_CONTENT))
                addView(ui.body(CalendarFormat.rowTitle(r) ?: "", 16f).apply { maxLines = 1; ellipsize = TextUtils.TruncateAt.END },
                    LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
            timeline.addView(ui.hairline())
        }
        rows.forEachIndexed { i, r ->
            if (i > 0) timeline.addView(ui.hairline())
            timeline.addView(if (r.current) currentBox(r, recording) else row(r))
        }
    }

    /** One row: time and title, room and attendees, then the four marks in fixed places. */
    private fun row(r: AgendaRow): View = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, ui.dim(14), 0, ui.dim(14))
        isClickable = true
        setOnClickListener {
            when {
                r.isEvent -> actions.onEvent(r)
                r.unscheduled && hasLinks -> actions.onUnscheduled(r)
                else -> r.meetingId?.let(actions::onMeeting)
            }
        }
        addView(LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            addView(ui.eyebrow(CalendarFormat.range(r.startMs, r.endMs, zone)))
            val title = CalendarFormat.rowTitle(r)
            addView(ui.body(title ?: Format.UNTITLED, 17f, bold = title != null).apply {
                maxLines = 1; ellipsize = TextUtils.TruncateAt.END
                if (title == null) { typeface = ui.serifItalic; setTextColor(ui.textMuted) }
            }, ui.fullWidth().apply { topMargin = ui.dim(4) })
            val detail = CalendarFormat.detail(r)
            if (detail.isNotEmpty()) addView(ui.note(detail, 13f).apply { maxLines = 1; ellipsize = TextUtils.TruncateAt.END },
                ui.fullWidth().apply { topMargin = ui.dim(2) })
            // Length and cost, and for a recording outside any event the way to link it.
            val line = CalendarFormat.meetingLine(r, hasLinks)
            if (line.isNotEmpty()) addView(ui.eyebrow(line, ui.inkSoft).apply { isAllCaps = false; text = line },
                ui.fullWidth().apply { topMargin = ui.dim(4) })
        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        addView(marks(r.marks))
    }

    /**
     * ● ✎ ≡ ☐ n as small boxes in fixed slots (the mock-up): white on black when it exists,
     * outlined when not yet; the to-do box shows the open count and is left empty at none.
     */
    private fun marks(m: Marks): View = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        val box = ui.dim(30)
        CalendarFormat.marks(m).forEachIndexed { i, mark ->
            val wide = i == 3
            addView(TextView(a).apply {
                gravity = Gravity.CENTER; includeFontPadding = false
                textSize = ui.fs(if (wide) 12f else 14f); typeface = ui.serifBold
                if (mark == null) { visibility = View.INVISIBLE; return@apply }
                val (glyph, filled) = mark
                text = glyph
                setTextColor(if (filled) Color.WHITE else ui.ink)
                background = ui.outline(ui.dim(4), if (filled) ui.ink else Color.WHITE)
            }, LinearLayout.LayoutParams(if (wide) ui.dim(50) else box, box).apply { marginStart = ui.dim(6) })
        }
    }

    /** The current event: boxed, with "● Record" (or the running timer) and "From last time". */
    private fun currentBox(r: AgendaRow, recording: Boolean): View = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        background = ui.outline(ui.dim(6))
        setPadding(ui.dim(16), ui.dim(12), ui.dim(12), ui.dim(14))
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            topMargin = ui.dim(8); bottomMargin = ui.dim(8)
        }
        addView(row(r))
        addView(LinearLayout(a).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.eyebrow("Now", ui.ink), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            if (recording) {
                addView(ui.body("● 0:00:00", 18f, bold = true).also { timer = it })
            } else {
                addView(ui.pill("●  Record", filled = true, size = 16f) { actions.onRecordEvent(r) })
            }
        })
        if (r.fromLastTime.isNotEmpty()) {
            addView(ui.eyebrow("From last time", ui.inkSoft), ui.fullWidth().apply { topMargin = ui.dim(12) })
            r.fromLastTime.forEach { t -> addView(todoRow(t, showSource = false)) }
        }
    }

    /** F6-FR1's rule: one kind of to-do, from every meeting; each names its meeting and date. */
    fun showTodos(todos: List<TodoRef>, error: String? = null) {
        todoRows.removeAllViews()
        todoTitle.text = if (error != null) "Open to-dos" else "Open to-dos  ·  ${todos.size}"
        if (error != null) return empty(todoRows, "Couldn't read the to-dos: $error")
        if (todos.isEmpty()) return empty(todoRows, "Nothing open. To-dos appear here after a meeting is transcribed.")
        todos.forEachIndexed { i, t ->
            if (i > 0) todoRows.addView(ui.hairline())
            todoRows.addView(todoRow(t, showSource = true))
        }
    }

    private fun todoRow(t: TodoRef, showSource: Boolean): View = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, ui.dim(12), 0, ui.dim(12))
        addView(ui.checkbox(false).apply {
            isClickable = true
            setOnClickListener {
                // Ticked at once (one redraw); the row leaves when the list is re-read.
                isClickable = false; text = "✓"; setTextColor(Color.WHITE)
                background = ui.outline(ui.dim(3), ui.ink)
                actions.onTick(t)
            }
        })
        addView(LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.dim(14), 0, 0, 0)
            addView(ui.body(t.text, 16f))
            val meta = if (showSource) CalendarFormat.todoSource(t) else HomeRows.todo(t).meta
            if (meta.isNotEmpty()) addView(ui.note(meta, 13f).apply {
                maxLines = 1; ellipsize = TextUtils.TruncateAt.END
                if (showSource) { isClickable = true; setOnClickListener { actions.onTodoSource(t) } }
            }, ui.wrap().apply { topMargin = ui.dim(3) })
        }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
    }

    /** S1: no all-files access yet; why it is needed and Grant access, in place of the day. */
    fun showStorageNeeded(text: String, onGrant: () -> Unit) {
        timeline.removeAllViews(); todoRows.removeAllViews()
        timer = null
        timeline.addView(ui.eyebrow("All-files access"), ui.wrap().apply { topMargin = ui.dim(14) })
        timeline.addView(ui.body(text, 17f), ui.wrap().apply { topMargin = ui.dim(8) })
        timeline.addView(ui.pill("Grant access", size = 16f, onClick = onGrant), ui.wrap().apply { topMargin = ui.dim(16); bottomMargin = ui.dim(14) })
        empty(todoRows, "To-dos appear here once Meetings can read your recordings.")
    }

    companion object {
        /** A to-do's meeting, as the right column names it ("Weekly sync · Wed 7 Oct"). */
        fun untitled(t: TodoRef) = t.meetingTitle?.ifBlank { null } ?: Format.UNTITLED
    }
}
