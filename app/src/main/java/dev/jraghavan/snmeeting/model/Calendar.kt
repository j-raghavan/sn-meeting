package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.TemporalAdjusters
import java.util.Locale

/** F18-FR7's four marks: filled when it exists or is done, outlined when not yet. */
data class Marks(val recording: Boolean, val notes: Boolean, val minutes: Boolean, val todosOpen: Int, val todosTotal: Int) {
    companion object {
        val NONE = Marks(false, false, false, 0, 0)
        fun from(o: JSONObject?): Marks = if (o == null) NONE else Marks(
            o.bool("recording") ?: false, o.bool("notes") ?: false, o.bool("minutes") ?: false,
            o.int("todosOpen") ?: 0, o.int("todosTotal") ?: 0,
        )
    }
}

/** One row of a day: an event (with the meeting linked to it, if any) or a meeting. */
data class AgendaRow(
    val isEvent: Boolean,
    val eventKey: String?, val meetingId: String?, val title: String?,
    val startMs: Long, val endMs: Long, val allDay: Boolean,
    val location: String?, val people: String, val calendar: String?,
    val recurring: Boolean, val current: Boolean, val unscheduled: Boolean,
    val marks: Marks, val fromLastTime: List<TodoRef>,
    /** The (linked) meeting's length and cost, "—" for none (F16-FR5). */
    val durationMs: Long? = null, val cost: String = "—",
) {
    companion object {
        fun from(o: JSONObject): AgendaRow? {
            val event = o.str("kind") == "event"
            return AgendaRow(
                isEvent = event,
                eventKey = o.str("eventKey"), meetingId = o.str("meetingId"), title = o.str("title"),
                startMs = o.long("startMs") ?: return null, endMs = o.long("endMs") ?: 0,
                allDay = o.bool("allDay") ?: false,
                location = o.str("location"), people = o.strOr("people", ""), calendar = o.str("calendar"),
                recurring = o.bool("recurring") ?: false, current = o.bool("current") ?: false,
                unscheduled = o.bool("unscheduled") ?: false,
                marks = Marks.from(o.obj("marks")),
                fromLastTime = o.arr("fromLastTime").mapObj(TodoRef::from),
                durationMs = o.long("durationMs"), cost = Spend.label(o.obj("spend")),
            )
        }
    }
}

data class AgendaDay(val day: LocalDate, val dot: Boolean, val allDay: List<AgendaRow>, val rows: List<AgendaRow>) {
    companion object {
        fun from(o: JSONObject): AgendaDay? {
            val day = try { LocalDate.parse(o.str("day")) } catch (e: Exception) { return null }
            return AgendaDay(
                day = day,
                dot = o.bool("dot") ?: false,
                allDay = o.arr("allDay").mapObj(AgendaRow::from),
                rows = o.arr("rows").mapObj(AgendaRow::from),
            )
        }
    }
}

/** `nativeAgenda`'s reply. */
data class Agenda(val hasLinks: Boolean, val updatedAtMs: Long?, val failed: Int, val days: List<AgendaDay>) {
    fun day(d: LocalDate) = days.firstOrNull { it.day == d }

    companion object {
        fun from(o: JSONObject): Agenda = Agenda(
            hasLinks = o.bool("hasLinks") ?: false,
            updatedAtMs = o.long("updatedAtMs"),
            failed = o.arr("failed")?.length() ?: 0,
            days = o.arr("days").mapObj(AgendaDay::from),
        )
    }
}

/** A calendar link as Settings → Calendar shows it (F18-FR13): never its URL. */
data class CalendarLink(
    val linkId: String, val name: String, val elided: String, val lastOk: Long?, val status: String,
    val reason: String?, val eventsThisWeek: Int, val skipped: Int,
) {
    companion object {
        fun from(o: JSONObject): CalendarLink? = o.str("linkId")?.let { id -> CalendarLink(
            linkId = id, name = o.strOr("name", ""), elided = o.strOr("elided", ""),
            lastOk = o.long("lastOk"), status = o.strOr("status", "never"), reason = o.str("reason"),
            eventsThisWeek = o.int("eventsThisWeek") ?: 0, skipped = o.int("skipped") ?: 0,
        ) }
    }
}

/** A meeting in "Earlier in this series". */
data class SeriesMeeting(val meetingId: String, val title: String?, val startedAt: String, val openTodos: Int) {
    companion object {
        fun from(o: JSONObject): SeriesMeeting? = o.str("meetingId")?.let { id -> SeriesMeeting(
            meetingId = id, title = o.str("title"),
            startedAt = o.strOr("startedAt", ""), openTodos = o.int("openTodos") ?: 0,
        ) }
    }
}

/** `nativeEvent`'s reply: the event sheet (F18-FR8). */
data class EventSheet(
    val eventKey: String, val title: String, val startMs: Long, val endMs: Long, val allDay: Boolean,
    val location: String?, val attendees: List<String>, val agenda: String?, val calendar: String?,
    val recurring: Boolean, val tzUnknown: Boolean,
    val templateId: String, val templateKeyword: String?,
    val series: List<SeriesMeeting>, val fromLastTime: List<TodoRef>, val linked: List<String>,
) {
    companion object {
        fun from(o: JSONObject): EventSheet? {
            val e = o.obj("event") ?: return null
            val t = o.obj("template")
            val people = e.arr("attendees").mapObj { a -> a.str("name") ?: a.str("email") }
            return EventSheet(
                eventKey = e.str("eventKey") ?: return null, title = e.strOr("title", ""),
                startMs = e.long("startMs") ?: return null, endMs = e.long("endMs") ?: 0,
                allDay = e.bool("allDay") ?: false, location = e.str("location"), attendees = people,
                agenda = e.str("agenda"), calendar = o.str("calendar"),
                recurring = e.bool("recurring") ?: false, tzUnknown = e.bool("tzUnknown") ?: false,
                templateId = t?.str("id") ?: "general", templateKeyword = t?.str("keyword"),
                series = o.arr("series").mapObj(SeriesMeeting::from),
                fromLastTime = o.arr("fromLastTime").mapObj(TodoRef::from),
                linked = o.arr("linked").strings(),
            )
        }
    }
}

/** What Layout A says, as text (F18-FR7), testable on the JVM. */
object CalendarFormat {
    private val hm = DateTimeFormatter.ofPattern("HH:mm", Locale.UK)

    fun time(ms: Long, zone: ZoneId): String = Instant.ofEpochMilli(ms).atZone(zone).format(hm)

    /** "10:00–10:30"; one time when it has no length. */
    fun range(startMs: Long, endMs: Long, zone: ZoneId): String =
        if (endMs <= startMs) time(startMs, zone) else "${time(startMs, zone)}–${time(endMs, zone)}"

    /** The header's date: "Thu 9 Oct". */
    fun headerDate(d: LocalDate): String = d.format(DateTimeFormatter.ofPattern("EEE d MMM", Locale.UK))

    /** "Today", "Tomorrow", "Yesterday", else "Thu 8 Oct" (the left column's heading). */
    fun dayTitle(d: LocalDate, today: LocalDate): String = when (d) {
        today -> "Today"
        today.plusDays(1) -> "Tomorrow"
        today.minusDays(1) -> "Yesterday"
        else -> headerDate(d)
    }

    /** Mon–Sun of the week holding [d]. */
    fun week(d: LocalDate): List<LocalDate> {
        val mon = d.with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY))
        return (0L until 7L).map { mon.plusDays(it) }
    }

    /** "MON" … "SUN". */
    fun weekday(d: LocalDate): String = d.dayOfWeek.getDisplayName(java.time.format.TextStyle.SHORT, Locale.UK).uppercase(Locale.UK)

    /** "UPDATED 09:58", "UPDATED 09:58 · 1 failed", or "NOT UPDATED YET". */
    fun updated(updatedAtMs: Long?, failed: Int, zone: ZoneId): String {
        val base = if (updatedAtMs == null) "Not updated yet" else "Updated ${time(updatedAtMs, zone)}"
        return if (failed > 0) "$base · $failed failed" else base
    }

    /**
     * F18-FR7's marks in their fixed slots, as boxes: (glyph, filled), filled (white on black)
     * when it exists, outlined when not yet. The fourth is "☐ n", the open to-dos, and is null
     * (an empty slot) when none are open.
     */
    fun marks(m: Marks): List<Pair<String, Boolean>?> = listOf(
        "●" to m.recording,
        "✎" to m.notes,
        "≡" to m.minutes,
        if (m.todosOpen > 0) "☐ ${m.todosOpen}" to false else null,
    )

    /** A row's main line: the event's title, or the meeting's (null for an untitled one). */
    fun rowTitle(r: AgendaRow): String? = if (r.isEvent) r.title?.ifBlank { null } ?: "(No title)" else r.title?.ifBlank { null }

    /** "0:38", hours and minutes; a recording under a minute reads "0:01". */
    fun length(ms: Long): String {
        val min = ((ms + 59_999) / 60_000).coerceAtLeast(if (ms > 0) 1 else 0)
        return "%d:%02d".format(Locale.ROOT, min / 60, min % 60)
    }

    /**
     * A recording's secondary line: "0:38 · $0.01" (the cost left out when nothing is known),
     * then "Unscheduled · tap to link" when it is outside any event and there are events to
     * link to (F18-FR10).
     */
    fun meetingLine(r: AgendaRow, hasLinks: Boolean): String = listOfNotNull(
        r.durationMs?.let(::length),
        r.cost.removePrefix("≈ ").takeIf { it != "—" && it.isNotBlank() },
        if (!r.isEvent && r.unscheduled && hasLinks) "Unscheduled · tap to link" else null,
    ).joinToString(" · ")

    /** The italic line under a row: room and attendees, as one line. */
    fun detail(r: AgendaRow): String = listOfNotNull(r.location?.ifBlank { null }, r.people.ifBlank { null }).joinToString(" · ")

    /** The row to box: the current event (F18-FR7). */
    fun current(rows: List<AgendaRow>): AgendaRow? = rows.firstOrNull { it.current }

    /** The minute the boxed row could change: the next start or end after [nowMs], so Home
     *  redraws only then (never on a faster timer, NFR2). */
    fun nextChange(rows: List<AgendaRow>, nowMs: Long): Long? =
        rows.filter { it.isEvent && !it.allDay }.flatMap { listOf(it.startMs, it.endMs) }.filter { it > nowMs }.minOrNull()

    /** F18-FR10's picker: the day's events, the ones overlapping the recording first. */
    fun pickerOrder(events: List<AgendaRow>, startMs: Long, endMs: Long): List<AgendaRow> {
        val end = maxOf(endMs, startMs + 1)
        val (over, rest) = events.filter { it.isEvent }.partition { it.startMs < end && it.endMs > startMs }
        return over.sortedBy { it.startMs } + rest.sortedBy { it.startMs }
    }

    /** A to-do's source on Home's right column: "Weekly sync · Wed 7 Oct · Due Fri 9 Oct". */
    fun todoSource(t: TodoRef): String = listOfNotNull(
        t.meetingTitle?.ifBlank { null } ?: Format.UNTITLED,
        Format.day(t.meetingStartedAt).ifEmpty { null },
        t.due?.let { "Due " + Format.due(it) },
    ).joinToString(" · ")

    /** The settings choices for the refresh interval (F18-FR4). */
    val INTERVALS = listOf(15 to "Every 15 minutes", 30 to "Every 30 minutes", 60 to "Every hour", 240 to "Every 4 hours", 0 to "Manual only")

    /** A link's status line in Settings → Calendar (F18-FR13). */
    fun linkStatus(l: CalendarLink, zone: ZoneId): String {
        val last = l.lastOk?.let { "updated ${Instant.ofEpochMilli(it).atZone(zone).format(DateTimeFormatter.ofPattern("EEE d MMM HH:mm", Locale.UK))}" }
        val state = when (l.status) {
            "ok" -> "OK"
            "never" -> "Not fetched yet"
            else -> l.reason ?: "Failed"
        }
        val week = Format.plural(l.eventsThisWeek, "event") + " this week"
        val skipped = if (l.skipped > 0) Format.plural(l.skipped, "event") + " couldn't be read" else null
        return listOfNotNull(state, last, week, skipped).joinToString(" · ")
    }

    const val HOW_TO = "Add a link from your computer: open MyStyle/Meetings/calendar-links.txt and paste your calendar's iCal link on its own line. " +
        "Google Calendar: Settings → your calendar → Integrate calendar → Secret address in iCal format. " +
        "Outlook: Settings → Calendar → Shared calendars → Publish a calendar → ICS. " +
        "Keep the link private: anyone with it can read your calendar."
    const val LIMIT = "Google and Outlook update these links on their own schedule, which can be hours behind."
    const val SYNC_NOTE = "Partner may copy MyStyle to your computer, so the links file travels with it."
}

/** F18-FR4: whether an automatic refresh may run now (manual Refresh always may). */
object CalendarRefresh {
    /**
     * Never while a provider job may be uploading audio on a metered connection [Recommendation].
     * Meetings doesn't hold ACCESS_NETWORK_STATE, so it can't tell a metered network apart: an
     * automatic refresh waits for every transcribe or summarise job (Refresh still runs).
     */
    fun autoAllowed(meetingJob: Boolean): Boolean = !meetingJob

    /** The interval in ms, or null for manual only. */
    fun intervalMs(refreshMin: Int): Long? = if (refreshMin <= 0) null else refreshMin * 60_000L
}

object CalendarPatch {
    fun refreshMin(m: Int) = """{"calendar":{"refreshMin":$m}}"""
    fun linkByTime(on: Boolean) = """{"calendar":{"linkByTime":$on}}"""
    fun eventInMinutes(on: Boolean) = """{"calendar":{"eventInMinutes":$on}}"""
    fun rules(rules: List<Pair<String, String>>) = """{"calendar":{"templateRules":[""" +
        rules.joinToString(",") { (k, t) -> """{"keyword":${CoreJson.quote(k)},"templateId":${CoreJson.quote(t)}}""" } + "]}}"
}

/** settings.calendar, as the shell reads it. */
data class CalendarPrefs(val refreshMin: Int = 30, val linkByTime: Boolean = true, val eventInMinutes: Boolean = true, val rules: List<Pair<String, String>> = emptyList()) {
    companion object {
        fun from(o: JSONObject?): CalendarPrefs = if (o == null) CalendarPrefs() else CalendarPrefs(
            refreshMin = o.int("refreshMin") ?: 30,
            linkByTime = o.bool("linkByTime") ?: true,
            eventInMinutes = o.bool("eventInMinutes") ?: true,
            rules = o.arr("templateRules").mapObj { r -> r.str("keyword")?.let { k -> k to (r.str("templateId") ?: "general") } },
        )
    }
}
