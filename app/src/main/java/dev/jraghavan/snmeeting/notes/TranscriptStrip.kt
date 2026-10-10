package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.model.LiveTranscript

/**
 * DEVICE-UNVERIFIED (refresh on the Manta). F17-FR6: the 3-line strip along the bottom of the
 * Notes tab, and the full transcript it opens over the lower half. It draws [StripModel] and
 * redraws only its own views, only when [StripModel.same] says the picture changed.
 */
class TranscriptStrip(private val a: Activity, private val ui: Inkwell, private val onToggle: () -> Unit) {
    private var model = StripModel()
    private val lines = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
    private val status = ui.eyebrow("", ui.inkSoft)
    private val header = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        addView(ui.eyebrow("Transcript"), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        addView(status)
    }
    private val scroll = ScrollView(a).apply { isVerticalScrollBarEnabled = false; addView(lines) }

    /** The strip; it fills the lower half when open. */
    val view: LinearLayout = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        setBackgroundColor(Color.WHITE)
        setPadding(ui.dim(20), ui.dim(10), ui.dim(20), ui.dim(12))
        addView(ui.hairline())
        addView(header, ui.fullWidth().apply { topMargin = ui.dim(8) })
        addView(scroll, ui.fullWidth().apply { topMargin = ui.dim(6) })
        isClickable = true
        setOnClickListener { onToggle() }
    }

    val state: StripModel get() = model

    fun show(next: StripModel) {
        if (model.same(next) && view.childCount > 0 && lines.childCount > 0) { model = next; return }
        model = next
        view.visibility = if (next.visible) View.VISIBLE else View.GONE
        status.text = next.status.uppercase()
        lines.removeAllViews()
        val shown = next.lines
        if (shown.isEmpty()) {
            lines.addView(ui.note(LiveTranscript.EMPTY, 15f, ui.textMuted))
            return
        }
        shown.forEach { l ->
            lines.addView(TextView(a).apply {
                text = l.text
                textSize = ui.fs(15f)
                typeface = if (l.markId != null) ui.mono else ui.serif
                setTextColor(ui.ink)
                if (l.n == next.highlight) { background = ui.outline(ui.dim(4)); setPadding(ui.dim(8), ui.dim(4), ui.dim(8), ui.dim(4)) }
                maxLines = if (next.open) Int.MAX_VALUE else 1
                ellipsize = if (next.open) null else android.text.TextUtils.TruncateAt.END
            }, ui.fullWidth().apply { topMargin = ui.dim(4) })
        }
        if (next.open) scroll.post { scroll.fullScroll(View.FOCUS_DOWN) }
    }
}
