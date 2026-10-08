package dev.jraghavan.snmeeting.model

import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

object Format {
    /** `h:mm:ss`, as the recorder's timer and the transcript markers. */
    fun duration(ms: Long?): String {
        if (ms == null) return "—"
        val s = ms / 1000
        return "%d:%02d:%02d".format(Locale.ROOT, s / 3600, (s / 60) % 60, s % 60)
    }

    /** startedAt carries the device's offset at recording time; show that wall time, not a re-zoned one. */
    private fun at(iso: String): OffsetDateTime? = try { OffsetDateTime.parse(iso) } catch (e: Exception) { null }
    fun day(iso: String) = at(iso)?.format(DateTimeFormatter.ofPattern("EEE d MMM", Locale.UK)) ?: ""
    fun dayLong(iso: String) = at(iso)?.format(DateTimeFormatter.ofPattern("EEEE, d MMMM", Locale.UK)) ?: ""
    fun time(iso: String) = at(iso)?.format(DateTimeFormatter.ofPattern("HH:mm", Locale.UK)) ?: ""

    /** A to-do's ISO due date as "Fri 9 Oct"; anything else as it is. */
    fun due(iso: String): String = try {
        LocalDate.parse(iso).format(DateTimeFormatter.ofPattern("EEE d MMM", Locale.UK))
    } catch (e: Exception) {
        iso
    }

    fun providerName(p: String) = when (p) { "openai" -> "OpenAI"; "gemini" -> "Google Gemini"; "anthropic" -> "Anthropic"; else -> p }

    fun plural(n: Int, one: String, many: String = one + "s") = if (n == 1) "1 $one" else "$n $many"

    const val UNTITLED = "Untitled meeting"
}

/** F6-FR1: what a Home row shows, from the core's rows. */
object HomeRows {
    data class Meeting(
        val id: String, val title: String, val untitled: Boolean, val stamp: String,
        val detail: String, val chip: String, val attention: Boolean,
    )

    data class Todo(val meetingId: String, val todoId: String, val text: String, val meta: String)

    fun chip(status: Status, source: Source): Pair<String, Boolean> = when {
        source == Source.CORRUPT -> "Unreadable" to true
        else -> when (status) {
            Status.RECORDING -> "Recording" to false
            Status.RECORDED -> "Recorded" to false
            Status.TRANSCRIBING -> "Transcribing…" to false
            Status.TRANSCRIBED, Status.DONE -> "Transcribed" to false
            Status.FAILED -> "Failed" to true
            Status.UNKNOWN -> "Unknown" to true
        }
    }

    fun meeting(m: MeetingSummary): Meeting {
        val todos = when {
            m.status != Status.DONE && m.status != Status.TRANSCRIBED -> ""
            m.openTodoCount == 0 -> "  ·  no open to-dos"
            else -> "  ·  " + Format.plural(m.openTodoCount, "open to-do")
        }
        val (label, attention) = chip(m.status, m.source)
        return Meeting(
            id = m.id,
            title = m.title?.takeIf { it.isNotBlank() } ?: Format.UNTITLED,
            untitled = m.title.isNullOrBlank(),
            stamp = "${Format.day(m.startedAt)}  ·  ${Format.time(m.startedAt)}",
            detail = Format.duration(m.durationMs) + todos,
            chip = label, attention = attention,
        )
    }

    /** "Wed 7 Oct · Due Fri 9 Oct · Me": the meeting's date, then what is known. */
    fun todo(t: TodoRef): Todo {
        val parts = listOfNotNull(
            Format.day(t.meetingStartedAt).ifEmpty { null },
            t.due?.let { "Due " + Format.due(it) },
            t.owner?.takeIf { it.isNotBlank() }?.replaceFirstChar { it.uppercase() },
        )
        return Todo(t.meetingId, t.todoId, t.text, parts.joinToString("  ·  "))
    }
}
