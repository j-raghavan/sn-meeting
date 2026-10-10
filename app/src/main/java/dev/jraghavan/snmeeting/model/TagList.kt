package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** F20-FR8: one recording under a selected tag. */
data class TagRecording(
    val id: String, val title: String?, val startedAt: String, val startedAtMs: Long,
    val durationMs: Long?, val otherTags: List<String>,
) {
    /** "Fri 9 Oct · 17:14 · 1:39". */
    val stamp: String get() = listOf(Format.day(startedAt), Format.time(startedAt), durationMs?.let(CalendarFormat::length).orEmpty())
        .filter { it.isNotEmpty() }.joinToString("  ·  ")
    val shownTitle: String get() = title?.ifBlank { null } ?: Format.UNTITLED
    /** "#local-ai  #microsoft". */
    val tagLine: String get() = otherTags.joinToString("  ") { Tags.shown(it) }
}

/** `nativeTagMeetings`: the tag's recordings, newest first, in pages of the e-ink list. */
data class TagList(val tag: String, val meetings: List<TagRecording>) {
    /** "#gpu · 5 recordings · newest first". */
    val header: String get() = "${Tags.shown(tag)}  ·  ${Format.plural(meetings.size, "recording")}  ·  newest first"

    fun pages(size: Int = PAGE): Int = maxOf(1, (meetings.size + size - 1) / size)

    /** Page [p] (0-based, clamped). */
    fun page(p: Int, size: Int = PAGE): List<TagRecording> {
        val at = p.coerceIn(0, pages(size) - 1) * size
        return meetings.drop(at).take(size)
    }

    /** "1 / 3". */
    fun pageLabel(p: Int, size: Int = PAGE): String = "${p.coerceIn(0, pages(size) - 1) + 1} / ${pages(size)}"

    companion object {
        /** Rows per screen of the list (no live scrolling list on e-ink). */
        const val PAGE = 4

        fun from(o: JSONObject): TagList? { return TagList(
            tag = o.str("tag") ?: return null,
            meetings = o.arr("meetings").mapObj { m ->
                TagRecording(m.str("id") ?: return@mapObj null, m.str("title"), m.strOr("startedAt", ""),
                    m.long("startedAtMs") ?: 0, m.long("durationMs"), m.arr("tags").strings())
            }.sortedByDescending { it.startedAtMs },
        ) }
    }
}
