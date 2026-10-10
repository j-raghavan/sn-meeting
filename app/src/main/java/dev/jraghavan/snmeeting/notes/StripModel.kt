package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.model.LiveLine
import dev.jraghavan.snmeeting.model.LiveTranscript

/**
 * F17-FR6: the 3-line transcript strip along the bottom of the Notes tab. It shows the newest
 * lines and marks, or (after a Listen tap, F17-FR7) the lines ending at the one spoken then;
 * tapping it opens every line over the lower half, and tapping the header closes it. Focus
 * hides it. [same] says whether a new reply changes what is drawn: the strip redraws only
 * when lines arrive or its status changes, never on a timer (F17-FR11).
 */
data class StripModel(
    val transcript: LiveTranscript? = null,
    val open: Boolean = false,
    val focus: Boolean = false,
    /** The line Listen highlighted (its `n`), or null for the newest. */
    val highlight: Int? = null,
) {
    val visible get() = !focus

    /** The lines drawn: all of them when open, else three, ending at the highlight or newest. */
    val lines: List<LiveLine> get() {
        val all = transcript?.lines.orEmpty()
        if (open) return all
        val end = highlight?.let { n -> all.indexOfFirst { it.n == n }.takeIf { it >= 0 } } ?: all.lastIndex
        return all.subList((end - LiveTranscript.STRIP_LINES + 1).coerceAtLeast(0), end + 1)
    }

    /** "Offline · 4 waiting", "Transcribing…" or nothing, at the strip's right end. */
    val status: String get() = transcript?.status.orEmpty()

    /** Whether [next] would draw the same strip as this one. */
    fun same(next: StripModel): Boolean =
        next.visible == visible && next.open == open && next.lines == lines && next.status == status &&
            next.highlight == highlight

    fun toggled() = copy(open = !open)

    companion object {
        /**
         * The line spoken at [ms]: the last transcript line (not a mark) whose estimated time is
         * at or before it (F15-FR6's times; F17-FR7). Null before the first line.
         */
        fun lineAt(lines: List<LiveLine>, ms: Long): Int? =
            lines.filter { it.markId == null && it.atMs <= ms }.maxByOrNull { it.atMs }?.n
    }
}
