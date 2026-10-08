package dev.jraghavan.snmeeting

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView

/**
 * One meeting (spec F7) in the Inkwell style (F9): play, transcribe / extract again, to-dos,
 * and the transcript paged for e-ink. Shows [MockData] until the Rust core (F3–F5) exists.
 */
class MeetingActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var meeting: MockMeeting
    private lateinit var transcriptBody: LinearLayout
    private lateinit var pageLabel: TextView
    private lateinit var actionNote: TextView
    private var page = 0
    private var playing = false
    private var templateId = MockSettings.defaultTemplate
    private var pickingTemplate = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        meeting = MockData.byId(intent.getStringExtra(EXTRA_ID)) ?: run { finish(); return }
        setContentView(ui.page(buildColumn()))
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun buildColumn(): LinearLayout = ui.column().apply {
        // Header: back link left, small wordmark centred.
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.eyebrow("←  All meetings").apply {
                isClickable = true; setOnClickListener { finish() }
                setPadding(0, ui.dim(8), ui.dim(8), ui.dim(8))
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.wordmark("Meetings", 30f))
            addView(View(this@MeetingActivity), LinearLayout.LayoutParams(0, 1, 1f))
        }, ui.fullWidth())

        addView(ui.spacer(ui.dim(36)))
        addView(ui.body(meeting.displayTitle, 30f, bold = meeting.title != null).apply {
            gravity = Gravity.CENTER
            if (meeting.title == null) { setTextColor(ui.textMuted); typeface = ui.serifItalic }
        })
        addView(ui.eyebrow("${meeting.dayLong}  ·  ${meeting.time}").apply {
            gravity = Gravity.CENTER; setPadding(0, ui.dim(10), 0, 0)
        })
        addView(ui.eyebrow("${meeting.duration}  ·  ${meeting.segments} segment${if (meeting.segments == 1) "" else "s"}").apply {
            gravity = Gravity.CENTER; setPadding(0, ui.dim(4), 0, 0)
        })
        addView(ui.note(meeting.mic).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(6), 0, 0) })

        addView(ui.spacer(ui.dim(32)))
        addView(playerCard())

        addView(ui.spacer(ui.dim(20)))
        addView(templateCard())
        addView(ui.spacer(ui.dim(24)))
        addView(actions())

        if (meeting.status == MockStatus.DONE) {
            addView(ui.spacer(ui.dim(36)))
            addView(ui.eyebrowItalic("To-dos from this meeting"))
            addView(ui.spacer(ui.dim(14)))
            addView(todoCard())

            addView(ui.spacer(ui.dim(36)))
            addView(ui.eyebrowItalic("Transcript"))
            addView(ui.spacer(ui.dim(14)))
            addView(transcriptCard())
        }

        addView(ui.spacerWeighted(ui.phi))
        addView(ui.spacer(ui.dim(32)))
        addView(ui.eyebrow("Delete meeting", ui.textMuted).apply {
            gravity = Gravity.CENTER; setPadding(0, ui.dim(8), 0, ui.dim(20))
            isClickable = true
        })
        addView(ui.closingMark())
    }

    /** Play all segments as one recording (F7-FR1). */
    private fun playerCard(): View = ui.card().apply {
        addView(ui.eyebrow("Recording"))
        lateinit var position: TextView
        val play = ui.pill("▶  Play", size = 17f) {}
        play.setOnClickListener {
            playing = !playing
            ui.stylePill(play, if (playing) "❚❚  Pause" else "▶  Play", filled = playing)
        }
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, ui.dim(12), 0, 0)
            addView(play)
            position = ui.body("0:00:00  /  ${meeting.duration}", 18f).apply { setPadding(ui.dim(20), 0, 0, 0) }
            addView(position)
        })
    }

    /** The meeting's template, with an inline picker (F7-FR2, F11-FR6); no dialog, one redraw per tap. */
    private fun templateCard(): View = ui.card(padV = ui.dim(12)).apply {
        val current = MockTemplates.byId(templateId) ?: MockTemplates.all.first()
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(LinearLayout(this@MeetingActivity).apply {
                orientation = LinearLayout.VERTICAL
                addView(ui.eyebrow("Template"))
                addView(ui.body(current.name, 18f, bold = true), ui.wrap().apply { topMargin = ui.dim(4) })
                addView(ui.note(current.description, 13f), ui.wrap().apply { topMargin = ui.dim(2) })
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.pill(if (pickingTemplate) "Done" else "Change", size = 15f) { pickingTemplate = !pickingTemplate; rebuild() })
        })
        if (pickingTemplate) {
            addView(ui.spacer(ui.dim(8)))
            MockTemplates.all.forEach { t ->
                addView(ui.hairline())
                addView(ui.choiceRow(t.name, t.description, t.id == current.id) { templateId = t.id; pickingTemplate = false; rebuild() })
            }
        }
    }

    private fun rebuild() = setContentView(ui.page(buildColumn()))

    /** The one action this meeting's status allows (F7-FR2). */
    private fun actions(): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        actionNote = ui.note("", 15f).apply { gravity = Gravity.CENTER }
        when (meeting.status) {
            MockStatus.RECORDED -> {
                addView(ui.pill("Transcribe", filled = true) { actionNote.text = "Transcription isn't connected yet." }, ui.wrap())
                actionNote.text = "Not transcribed yet. Audio is sent to OpenAI using your key."
            }
            MockStatus.TRANSCRIBING -> {
                addView(ui.chip("Transcribing 2 of 3"), ui.wrap())
                addView(ui.pill("Cancel", size = 15f) { actionNote.text = "Cancelling isn't connected yet." }, ui.wrap().apply { topMargin = ui.dim(12) })
                actionNote.text = "You can leave this screen; it carries on."
            }
            MockStatus.FAILED -> {
                addView(ui.pill("Transcribe", filled = true) { actionNote.text = "Transcription isn't connected yet." }, ui.wrap())
                actionNote.text = meeting.error ?: ""
            }
            MockStatus.DONE -> {
                addView(ui.pill("Summarise again") { actionNote.text = "Summarising isn't connected yet." }, ui.wrap())
                actionNote.text = "Ticked to-dos stay ticked."
            }
        }
        addView(actionNote, ui.wrap().apply { topMargin = ui.dim(12) })
    }

    private fun todoCard(): View = ui.card(padV = ui.dim(6)).apply {
        if (meeting.todos.isEmpty()) {
            addView(ui.note("No to-dos found in this meeting.").apply { setPadding(0, ui.dim(14), 0, ui.dim(14)) })
        }
        meeting.todos.forEachIndexed { i, t ->
            if (i > 0) addView(ui.hairline())
            addView(LinearLayout(this@MeetingActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(14), 0, ui.dim(14))
                val box = ui.checkbox(t.done)
                val label = ui.body(t.text, 17f)
                box.isClickable = true
                box.setOnClickListener {
                    t.done = !t.done
                    box.text = if (t.done) "✓" else ""
                    box.background = ui.outline(ui.dim(3), if (t.done) ui.ink else android.graphics.Color.WHITE)
                    label.setTextColor(if (t.done) ui.textMuted else ui.ink)
                }
                if (t.done) label.setTextColor(ui.textMuted)
                addView(box)
                addView(LinearLayout(this@MeetingActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(ui.dim(16), 0, 0, 0)
                    addView(label)
                    addView(ui.note(t.meta, 13f), ui.wrap().apply { topMargin = ui.dim(3) })
                }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
        }
    }

    /** Transcript paged for e-ink: whole pages, no scrolling inside the text (F7-FR3). */
    private fun transcriptCard(): View = ui.card(padV = ui.dim(22)).apply {
        transcriptBody = LinearLayout(this@MeetingActivity).apply { orientation = LinearLayout.VERTICAL }
        addView(transcriptBody, ui.fullWidth())
        addView(ui.spacer(ui.dim(18)))
        addView(ui.hairline())
        pageLabel = ui.eyebrow("").apply { gravity = Gravity.CENTER }
        addView(LinearLayout(this@MeetingActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, ui.dim(14), 0, 0)
            addView(ui.pill("‹  Previous", size = 15f) { turn(-1) })
            addView(pageLabel, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.pill("Next  ›", size = 15f) { turn(1) })
        }, ui.fullWidth())
        showPage()
    }

    private val pages get() = meeting.transcript.chunked(PARAGRAPHS_PER_PAGE)

    private fun turn(delta: Int) {
        val next = (page + delta).coerceIn(0, pages.size - 1)
        if (next != page) { page = next; showPage() }
    }

    private fun showPage() {
        transcriptBody.removeAllViews()
        pages.getOrNull(page)?.forEachIndexed { i, (marker, text) ->
            if (marker != null) {
                transcriptBody.addView(ui.eyebrow("[$marker]"), ui.wrap().apply { topMargin = if (i > 0) ui.dim(18) else 0 })
            }
            transcriptBody.addView(ui.body(text, 17f).apply { setLineSpacing(0f, 1.35f) },
                ui.fullWidth().apply { topMargin = ui.dim(8) })
        }
        pageLabel.text = "PAGE ${page + 1} OF ${pages.size}"
    }

    companion object {
        const val EXTRA_ID = "meeting_id"
        private const val PARAGRAPHS_PER_PAGE = 3
    }
}
