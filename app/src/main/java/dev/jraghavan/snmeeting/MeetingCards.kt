package dev.jraghavan.snmeeting

import android.app.Activity
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.SummaryCard
import dev.jraghavan.snmeeting.model.Todo
import dev.jraghavan.snmeeting.model.TranscriptPages

/** The meeting screen's summary, to-do and transcript cards (F7-FR3), in the Inkwell style. */
class MeetingCards(private val a: Activity, private val ui: Inkwell) {

    /** This meeting's to-dos, open and done; [onTick] stores the new state. */
    fun todos(todos: List<Todo>, onTick: (Todo) -> Unit): View = ui.card(padV = ui.dim(6)).apply {
        if (todos.isEmpty()) {
            addView(ui.note("No to-dos found in this meeting.").apply { setPadding(0, ui.dim(14), 0, ui.dim(14)) })
        }
        todos.forEachIndexed { i, t ->
            if (i > 0) addView(ui.hairline())
            addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                val label = ui.body(t.text, 17f).apply { if (t.done) setTextColor(ui.textMuted) }
                addView(ui.checkbox(t.done).apply {
                    isClickable = true
                    setOnClickListener {
                        // Shown at once (one redraw); stored by the core, then re-read.
                        isClickable = false
                        val done = !t.done
                        text = if (done) "✓" else ""
                        setTextColor(Color.WHITE)
                        background = ui.outline(ui.dim(3), if (done) ui.ink else Color.WHITE)
                        label.setTextColor(if (done) ui.textMuted else ui.ink)
                        onTick(t)
                    }
                })
                addView(LinearLayout(a).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(ui.dim(16), 0, 0, 0)
                    addView(label)
                    TranscriptPages.todoMeta(t)?.let { addView(ui.note(it, 13f), ui.wrap().apply { topMargin = ui.dim(3) }) }
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
        }
    }

    /**
     * S5: the summary, then Decisions as bullets, then each section under its mono eyebrow
     * heading. Static text in one card: the page scrolls, the card never does (e-ink).
     */
    /** F17-FR10: what a tap on the minutes corrects (null: they can't be edited now). */
    interface Edits {
        fun summary(current: String)
        /** [i] is the decision's index in the sidecar; [i] = count adds one. */
        fun decision(i: Int, current: String)
        fun section(i: Int, heading: String, current: String)
    }

    fun summary(c: SummaryCard, edits: Edits? = null): View = ui.card(padV = ui.dim(22)).apply {
        var first = true
        /** The space above a block: none for the first. */
        fun top(): Int = if (first) { first = false; 0 } else ui.dim(22)
        fun TextView.tap(f: () -> Unit) = apply { if (edits != null) { isClickable = true; setOnClickListener { f() } } }
        c.summary?.let { s -> addView(ui.body(s, 17f).apply { setLineSpacing(0f, 1.35f) }.tap { edits?.summary(s) }, ui.fullWidth().apply { topMargin = top() }) }
        if (c.decisions.isNotEmpty() || edits != null) {
            addView(ui.eyebrow("Decisions"), ui.wrap().apply { topMargin = top() })
            c.decisions.forEachIndexed { i, d ->
                addView(ui.body("•  $d", 17f).apply { setLineSpacing(0f, 1.3f) }.tap { edits?.decision(c.decisionIndex[i], d) },
                    ui.fullWidth().apply { topMargin = ui.dim(8) })
            }
            if (edits != null) addView(ui.note("＋  Add a decision", 15f, ui.textMuted).tap { edits.decision(c.decisionCount, "") },
                ui.fullWidth().apply { topMargin = ui.dim(10) })
        }
        c.sections.forEachIndexed { i, sec ->
            addView(ui.eyebrow(sec.heading), ui.wrap().apply { topMargin = top() })
            val body = sec.body.trim()
            addView(ui.body(body.ifEmpty { "—" }, 17f).apply { setLineSpacing(0f, 1.35f) }.tap { edits?.section(c.sectionIndex[i], sec.heading, body) },
                ui.fullWidth().apply { topMargin = ui.dim(8) })
        }
    }

    /**
     * The transcript in whole pages, no scrolling inside the text (e-ink): ‹ Previous and
     * Next › turn a page in place. [page] is the page to open on; [onTurn] remembers it.
     */
    /** [mark]: (page, paragraph) to outline, the one a search hit opened at (F19-FR3). */
    fun transcript(pages: List<List<TranscriptPages.Para>>, page: Int, mark: Pair<Int, Int>? = null, onTurn: (Int) -> Unit): View = ui.card(padV = ui.dim(22)).apply {
        val body = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
        val label: TextView = ui.eyebrow("").apply { gravity = Gravity.CENTER }
        var current = page.coerceIn(0, pages.size - 1)
        fun show() {
            body.removeAllViews()
            pages[current].forEachIndexed { i, p ->
                if (p.marker != null) {
                    body.addView(ui.eyebrow("[${p.marker}]"), ui.wrap().apply { topMargin = if (i > 0) ui.dim(18) else 0 })
                }
                body.addView(ui.body(p.text, 17f).apply {
                    setLineSpacing(0f, 1.35f)
                    if (mark == current to i) { background = ui.outline(ui.dim(4)); setPadding(ui.dim(10), ui.dim(8), ui.dim(10), ui.dim(8)) }
                }, ui.fullWidth().apply { topMargin = ui.dim(8) })
            }
            label.text = "PAGE ${current + 1} OF ${pages.size}"
        }
        fun turn(delta: Int) {
            val next = (current + delta).coerceIn(0, pages.size - 1)
            if (next != current) { current = next; onTurn(next); show() }
        }
        addView(body, ui.fullWidth())
        if (pages.size > 1) {
            addView(ui.spacer(ui.dim(18)))
            addView(ui.hairline())
            addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, 0)
                addView(ui.pill("‹  Previous", size = 15f) { turn(-1) })
                addView(label, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.pill("Next  ›", size = 15f) { turn(1) })
            }, ui.fullWidth())
        }
        show()
    }
}
