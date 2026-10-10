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
        /** F16-FR5: the meeting's cost, "—" with none. */
        val cost: String = "—",
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
            // N12: no summary yet, so "no open to-dos" would say more than is known.
            m.status == Status.TRANSCRIBED && m.openTodoCount == 0 -> "  ·  not summarised"
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
            cost = m.cost,
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

/** F7-FR5: the meeting screen's heading and the mono eyebrows under it. */
data class MeetingHeader(
    val title: String, val untitled: Boolean, val stamp: String, val length: String, val mic: String?, val note: String?,
    /** F16-FR5: what the meeting has cost so far ("≈ $0.41"), "—" with nothing recorded. */
    val cost: String = "—",
) {
    companion object {
        fun of(m: Sidecar, cost: String = "—"): MeetingHeader {
            val n = m.segments.size
            val mic = m.mic?.let { (if (it.kind == "bluetooth") "Bluetooth" else "USB") + " — " + it.name }
            return MeetingHeader(
                title = m.title?.takeIf { it.isNotBlank() } ?: Format.UNTITLED,
                untitled = m.title.isNullOrBlank(),
                stamp = listOf(Format.dayLong(m.startedAt), Format.time(m.startedAt)).filter { it.isNotEmpty() }.joinToString("  ·  "),
                length = "${Format.duration(m.durationMs)}  ·  ${Format.plural(n, "segment")}",
                mic = mic,
                note = m.note?.takeIf { it.isNotBlank() }?.let { "Recording ended: $it" },
                cost = cost,
            )
        }
    }
}

/** F7-FR3: what the meeting screen's Summary card shows; null when there is no summary yet. */
data class SummaryCard(val summary: String?, val decisions: List<String>, val sections: List<Section>) {
    companion object {
        fun of(m: Sidecar): SummaryCard? {
            val summary = m.summary?.trim()?.ifEmpty { null }
            val decisions = m.decisions.map { it.trim() }.filter { it.isNotEmpty() }
            val sections = m.sections.filter { it.heading.isNotBlank() || it.body.isNotBlank() }
            if (summary == null && decisions.isEmpty() && sections.isEmpty()) return null
            return SummaryCard(summary, decisions, sections)
        }
    }
}

/**
 * F7-FR2: the meeting's action row, from its status, whether its job is running here, and the
 * key status of each role's provider. Every status has an enabled action or shows progress (INV8).
 */
data class Actions(
    val primary: Job?, val primaryEnabled: Boolean, val progress: String?, val canCancel: Boolean,
    val note: String?, val canDelete: Boolean,
    /** F5-FR13: Transcribe again, beside Summarise again once there is a transcript. */
    val secondary: Job? = null,
) {
    enum class Job(val label: String) {
        TRANSCRIBE("Transcribe"), SUMMARISE("Summarise again"), TRANSCRIBE_AGAIN("Transcribe again"),
    }

    companion object {
        /** "Transcribing 2/3": the segment in flight (1-based) over the total. */
        fun transcribing(m: Sidecar): String {
            // F15-FR9: a near-live meeting counts its chunks ("Transcribing 41/42").
            if (m.chunkTotal > 0) return "Transcribing ${minOf(m.chunksDone + 1, m.chunkTotal)}/${m.chunkTotal}"
            val total = m.segments.size
            val n = minOf(m.segments.count { it.text != null } + 1, total)
            return "Transcribing $n/$total"
        }

        /** F15-FR8: the core's error on a recorded meeting with no audio left (recovery). */
        const val AUDIO_LOST = "Audio couldn't be recovered"

        /** Shown under a job that has run [SLOW_MS]: a reasoning model can take a while. */
        const val SLOW = "Reasoning models can take a minute or two."
        const val SLOW_MS = 20_000L
        private const val LEAVE = "You can leave this screen; it carries on."

        /**
         * F7-FR2's progress label with the job's elapsed time, in 10 s steps so the label
         * redraws at most every 10 s: "Summarising… 0:40". Nothing is added before 10 s.
         */
        fun withElapsed(label: String, elapsedMs: Long?): String {
            val s = (elapsedMs ?: 0) / 10_000 * 10
            return if (s <= 0) label else "%s %d:%02d".format(java.util.Locale.ROOT, label, s / 60, s % 60)
        }

        private fun note(elapsedMs: Long?, summarising: Boolean) =
            if (summarising && (elapsedMs ?: 0) >= SLOW_MS) "$SLOW $LEAVE" else LEAVE

        fun of(m: Sidecar, jobRunning: Boolean, transcribeKey: KeyStatus?, summariseKey: KeyStatus?, elapsedMs: Long? = null): Actions {
            fun problem(k: KeyStatus?) = when (k) {
                null -> "Checking the key…"
                is KeyStatus.Problem -> k.detail
                is KeyStatus.Found -> null
            }
            fun offer(job: Job, key: KeyStatus?, idle: String): Actions {
                val p = problem(key)
                return Actions(job, p == null, null, false, p ?: m.error ?: idle, true)
            }
            return when {
                m.status == Status.RECORDING -> Actions(null, false, "Recording…", false, null, false)
                m.status == Status.TRANSCRIBING && jobRunning ->
                    Actions(null, false, withElapsed(transcribing(m), elapsedMs), true, note(elapsedMs, summarising = false), false)
                // No job here (a screen restored after the process died): resume it (INV8).
                m.status == Status.TRANSCRIBING -> offer(Job.TRANSCRIBE, transcribeKey, "Transcription was interrupted.")
                jobRunning -> Actions(null, false, withElapsed("Summarising…", elapsedMs), true, note(elapsedMs, summarising = true), false)
                // F15-FR8: nothing to transcribe; say so, and leave Delete.
                m.status == Status.RECORDED && m.error == AUDIO_LOST -> Actions(null, false, null, false, "$AUDIO_LOST.", true)
                m.status == Status.RECORDED || (m.status == Status.FAILED && m.failedAt != FailedAt.SUMMARISING) ->
                    offer(Job.TRANSCRIBE, transcribeKey, "Not transcribed yet.")
                m.status == Status.TRANSCRIBED || m.status == Status.DONE -> {
                    val again = Job.TRANSCRIBE_AGAIN.takeIf { problem(transcribeKey) == null }
                    offer(Job.SUMMARISE, summariseKey, "Ticked to-dos stay ticked.").copy(secondary = again)
                }
                m.status == Status.FAILED -> offer(Job.SUMMARISE, summariseKey, "Ticked to-dos stay ticked.")
                else -> offer(Job.TRANSCRIBE, transcribeKey, "Not transcribed yet.") // an unknown status: start over
            }
        }
    }
}

/**
 * Security: the one-time notice before the first job to each provider. It names every provider
 * the job will send to (Transcribe also summarises when it can), and all of them are then
 * marked shown in settings.json.
 */
data class ProviderNotice(val message: String, val providers: List<String>) {
    companion object {
        const val TITLE = "Before it's sent"
        private const val RETENTION = " Each provider may keep what it receives under its own policy; Meetings can't prevent that."

        fun of(job: Actions.Job, s: SettingsView): ProviderNotice? {
            val summarise = s.effectiveSummarise.provider.takeIf { s.keys[it]?.usable == true }
            val transcribe = s.effectiveTranscribe.provider.takeIf { job != Actions.Job.SUMMARISE }
            val involved = listOfNotNull(transcribe, summarise).distinct()
            if (involved.isEmpty() || involved.all { s.noticeShown[it] == true }) return null
            val name = Format::providerName
            val text = when {
                transcribe == null -> "The transcript is sent to ${name(summarise!!)} using your key."
                summarise == null -> "The audio is sent to ${name(transcribe)} using your key."
                transcribe == summarise -> "The audio and its transcript are sent to ${name(transcribe)} using your key."
                else -> "The audio is sent to ${name(transcribe)}, and the transcript to ${name(summarise)}, using your keys."
            }
            return ProviderNotice(text + RETENTION, involved)
        }
    }
}

/** F7-FR3: the transcript as whole pages for e-ink, a [h:mm:ss] marker at each segment start. */
object TranscriptPages {
    data class Para(val marker: String?, val text: String)

    const val PAGE_CHARS = 1800

    fun paragraphs(segments: List<Segment>): List<Para> = segments.filter { !it.text.isNullOrBlank() }.flatMap { s ->
        s.text!!.split(Regex("\n\\s*\n")).map { it.trim() }.filter { it.isNotEmpty() }
            .mapIndexed { i, t -> Para(if (i == 0) Format.duration(s.startOffsetMs) else null, t) }
    }

    /**
     * F15-FR11: the paragraphs with each mark as its own line, `◆ [0:12:41] Mark`, before the
     * first paragraph that starts after it.
     */
    fun paragraphs(segments: List<Segment>, marks: List<Mark>): List<Para> = timed(segments, marks).map { it.second }

    /** Each paragraph, with its segment's start on the first of each segment's paragraphs. */
    private fun timed(segments: List<Segment>, marks: List<Mark>): List<Pair<Long?, Para>> {
        val left = ArrayDeque(marks.sortedBy { it.atMs })
        val out = mutableListOf<Pair<Long?, Para>>()
        for (s in segments.filter { !it.text.isNullOrBlank() }) {
            while (left.isNotEmpty() && left.first().atMs < s.startOffsetMs) out += null to Para(null, left.removeFirst().line)
            paragraphs(listOf(s)).forEachIndexed { i, p -> out += (if (i == 0) s.startOffsetMs else null) to p }
        }
        left.forEach { out += null to Para(null, it.line) }
        return out
    }

    /**
     * F19-FR3: the paragraph spoken at [ms] (the last segment starting at or before it, its first
     * paragraph), as (page, index on that page) of the pages built from [segments] and [marks];
     * null when none.
     */
    fun locate(segments: List<Segment>, ms: Long, budget: Int = PAGE_CHARS, marks: List<Mark> = emptyList()): Pair<Int, Int>? {
        val all = timed(segments, marks)
        var target: Int? = null
        all.forEachIndexed { i, (start, _) -> if (start != null && start <= ms) target = i }
        val t = target ?: return null
        var seen = 0
        pages(all.map { it.second }, budget).forEachIndexed { pi, page ->
            if (t < seen + page.size) return pi to (t - seen)
            seen += page.size
        }
        return null
    }

    /** Greedy by characters, so pages are even in height; a long paragraph gets a page to itself. */
    fun pages(paras: List<Para>, budget: Int = PAGE_CHARS): List<List<Para>> {
        val out = mutableListOf<MutableList<Para>>()
        var used = 0
        for (p in paras) {
            if (out.isEmpty() || (used + p.text.length > budget && out.last().isNotEmpty())) { out += mutableListOf<Para>(); used = 0 }
            out.last() += p; used += p.text.length
        }
        return out
    }

    /** "Due Fri 9 Oct · Me" under a meeting's to-do; null when neither is known. */
    fun todoMeta(t: Todo): String? = listOfNotNull(
        t.due?.let { "Due " + Format.due(it) },
        t.owner?.takeIf { it.isNotBlank() }?.replaceFirstChar { it.uppercase() },
    ).joinToString("  ·  ").ifEmpty { null }
}
