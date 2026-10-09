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
    fun summary(c: SummaryCard): View = ui.card(padV = ui.dim(22)).apply {
        var first = true
        /** The space above a block: none for the first. */
        fun top(): Int = if (first) { first = false; 0 } else ui.dim(22)
        c.summary?.let { addView(ui.body(it, 17f).apply { setLineSpacing(0f, 1.35f) }, ui.fullWidth().apply { topMargin = top() }) }
        if (c.decisions.isNotEmpty()) {
            addView(ui.eyebrow("Decisions"), ui.wrap().apply { topMargin = top() })
            c.decisions.forEach { d ->
                addView(ui.body("•  $d", 17f).apply { setLineSpacing(0f, 1.3f) }, ui.fullWidth().apply { topMargin = ui.dim(8) })
            }
        }
        c.sections.forEach { sec ->
            addView(ui.eyebrow(sec.heading), ui.wrap().apply { topMargin = top() })
            if (sec.body.isNotBlank()) {
                addView(ui.body(sec.body.trim(), 17f).apply { setLineSpacing(0f, 1.35f) }, ui.fullWidth().apply { topMargin = ui.dim(8) })
            }
        }
    }

    /**
     * The transcript in whole pages, no scrolling inside the text (e-ink): ‹ Previous and
     * Next › turn a page in place. [page] is the page to open on; [onTurn] remembers it.
     */
    fun transcript(pages: List<List<TranscriptPages.Para>>, page: Int, onTurn: (Int) -> Unit): View = ui.card(padV = ui.dim(22)).apply {
        val body = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
        val label: TextView = ui.eyebrow("").apply { gravity = Gravity.CENTER }
        var current = page.coerceIn(0, pages.size - 1)
        fun show() {
            body.removeAllViews()
            pages[current].forEachIndexed { i, p ->
                if (p.marker != null) {
                    body.addView(ui.eyebrow("[${p.marker}]"), ui.wrap().apply { topMargin = if (i > 0) ui.dim(18) else 0 })
                }
                body.addView(ui.body(p.text, 17f).apply { setLineSpacing(0f, 1.35f) }, ui.fullWidth().apply { topMargin = ui.dim(8) })
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
