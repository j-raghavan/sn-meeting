package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * F19: what the shell shows of Search. The core indexes, matches, groups and orders everything
 * (the search module); these only read nativeSearch / nativeIndexStatus and lay the results out.
 */
object SearchModel {
    /** The filter chips (F19-FR1), in order: the chip's label and the id nativeSearch takes. */
    enum class Filter(val label: String, val id: String, val noun: String) {
        ALL("All", "all", "anywhere"),
        NOTES("Notes", "notes", "in notes"),
        TRANSCRIPT("Transcript", "transcript", "in transcripts"),
        MINUTES("Minutes", "minutes", "in minutes"),
        TODOS("To-dos", "todos", "in to-dos"),
        EVENTS("Events", "events", "in events"),
        /** F20-FR5. */
        TAGS("Tags", "tags", "in tags"),
    }

    /** "No results for “budget” in notes" (F19-FR2: the active filter named). */
    fun noResults(query: String, f: Filter): String = "No results for “${query.trim()}” ${f.noun}"

    /** Groups asked for at first, and how many more each Show more adds (F19-FR2). */
    const val PAGE_GROUPS = 50

    /** "Indexing 120/340" while an update or rebuild runs; null when the index is ready. */
    fun indexing(status: IndexStatus?): String? = when {
        status == null -> null
        status.state == "indexing" && status.total > 0 -> "Indexing ${status.done}/${status.total}"
        status.state == "indexing" || status.state == "missing" -> "Still indexing…"
        else -> null
    }

    /** Settings → About after "Rebuild search index" (F19-FR5): what the core answered. */
    fun rebuilt(json: String): String = when (val r = CoreJson.obj(json) { o -> o }) {
        is CoreResult.Err -> "Couldn't rebuild the search index: ${r.message}"
        is CoreResult.Ok -> if (r.value.bool("busy") == true) "The search index is already being updated"
        else "Search index rebuilt  ·  " + Format.plural(r.value.int("indexed") ?: 0, "meeting or event", "meetings and events")
    }

    /** The group header's date and time (mono): "Fri 9 Oct 2026 · 10:00". */
    fun stamp(startMs: Long, zone: ZoneId = ZoneId.systemDefault()): String =
        Instant.ofEpochMilli(startMs).atZone(zone).format(DateTimeFormatter.ofPattern("EEE d MMM yyyy  ·  HH:mm", Locale.UK))

    /** A meeting with no title reads as on Home. */
    fun title(g: SearchGroup): String = g.title?.takeIf { it.isNotBlank() }
        ?: if (g.kind == "event") "Untitled event" else "Untitled meeting"

    /** The hit's mono line: "TRANSCRIPT  ·  0:12:41 ▶", with the F19-FR7 note for changed notes. */
    fun label(h: SearchHit): String = buildString {
        append(h.label)
        h.time?.let { append("  ·  $it ▶") }
        if (h.changed) append("  ·  (notes changed since recognised)")
    }

    /** Where a hit opens (F19-FR3). */
    sealed class Open {
        /** The meeting, cued to [atMs] when given, scrolled to [focus] (summary, todos, transcript),
         *  or (F19-FR3) on its Notes tab with the line's world box [outline] centred and outlined. */
        data class Meeting(val id: String, val atMs: Long?, val focus: String?, val outline: List<Double>? = null) : Open()
        /** An event with no meeting: its sheet (F18-FR8). */
        data class Event(val key: String, val title: String, val startMs: Long) : Open()
        /** F20-FR5: a tag hit opens the Tag Graph with that tag selected (F20-FR8). */
        data class Tag(val name: String) : Open()
    }

    /** ▶ on a notes hit plays from this long before the line was written (F19-FR3). */
    const val NOTES_LEAD_MS = 2_000L

    /**
     * F19-FR3. Transcript: the meeting at that moment. Minutes or to-do: the meeting at its
     * minutes or to-dos. Notes: the Notes tab centred on the line, its strokes outlined, and
     * playback cued 2 s before it was written (▶). Event: the event sheet. Tag (F20-FR5): the
     * Tag Graph with the tag selected.
     */
    fun open(g: SearchGroup, h: SearchHit): Open = when {
        g.kind == "event" -> Open.Event(g.id, title(g), g.startMs)
        h.kind == "tag" -> Open.Tag(h.unitId.removePrefix("tag:"))
        h.kind == "notes" -> Open.Meeting(g.id, h.atMs?.let { (it - NOTES_LEAD_MS).coerceAtLeast(0) }, "notes", h.bbox)
        h.kind == "transcript" -> Open.Meeting(g.id, h.atMs, "transcript")
        h.kind == "todo" -> Open.Meeting(g.id, null, "todos")
        else -> Open.Meeting(g.id, null, "summary")
    }

    /** "+ 12 more in this meeting" under a group whose hits were capped. */
    fun more(g: SearchGroup): String? = (g.hitCount - g.hits.size).takeIf { it > 0 }?.let { "+ $it more in this ${if (g.kind == "event") "event" else "meeting"}" }
}

/** One run of a snippet; [bold] runs are the matched terms (F9: bold, no colour). */
data class Span(val text: String, val bold: Boolean)

/** One hit of nativeSearch (F19-FR2). */
data class SearchHit(
    val kind: String, val label: String, val unitId: String, val atMs: Long?, val time: String?,
    val snippet: List<Span>, val changed: Boolean,
    /** A notes hit's line as a world box `[x0, y0, x1, y1]` (F19-FR3's outline). */
    val bbox: List<Double>? = null,
) {
    companion object {
        fun from(o: JSONObject): SearchHit? { return SearchHit(
            kind = o.str("kind") ?: return null,
            label = o.strOr("label", ""),
            unitId = o.strOr("unitId", ""),
            atMs = o.long("atMs"),
            time = o.str("time"),
            snippet = o.arr("snippet").mapObj { s -> s.str("text")?.let { Span(it, s.bool("bold") == true) } },
            changed = o.bool("changed") == true,
            bbox = o.arr("bbox")?.let { a -> (0 until a.length()).mapNotNull { (a.opt(it) as? Number)?.toDouble() } }?.takeIf { it.size == 4 },
        ) }
    }
}

/** A meeting's or an event's hits. */
data class SearchGroup(
    val kind: String, val id: String, val title: String?, val startMs: Long, val hitCount: Int, val hits: List<SearchHit>,
) {
    companion object {
        fun from(o: JSONObject): SearchGroup? { return SearchGroup(
            kind = o.str("kind") ?: return null,
            id = o.str("id") ?: return null,
            title = o.str("title"),
            startMs = o.long("startMs") ?: 0,
            hitCount = o.int("hitCount") ?: 0,
            hits = o.arr("hits").mapObj(SearchHit::from),
        ) }
    }
}

/** nativeSearch's reply. */
data class SearchResults(
    val query: String, val groups: List<SearchGroup>, val more: Boolean, val totalGroups: Int, val indexing: Boolean,
) {
    companion object {
        fun from(o: JSONObject): SearchResults = SearchResults(
            query = o.strOr("query", ""),
            groups = o.arr("groups").mapObj(SearchGroup::from),
            more = o.bool("more") == true,
            totalGroups = o.int("totalGroups") ?: 0,
            indexing = o.bool("indexing") == true,
        )
    }
}

/** nativeIndexStatus's reply. */
data class IndexStatus(val state: String, val done: Int, val total: Int) {
    companion object {
        fun from(o: JSONObject): IndexStatus? {
            return IndexStatus(o.str("state") ?: return null, o.int("done") ?: 0, o.int("total") ?: 0)
        }
    }
}
