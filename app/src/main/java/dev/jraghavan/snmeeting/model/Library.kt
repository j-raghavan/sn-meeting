package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** The sidecar status machine's states, as the core writes them (spec Data Model). */
enum class Status(val wire: String) {
    RECORDING("recording"), RECORDED("recorded"), TRANSCRIBING("transcribing"),
    TRANSCRIBED("transcribed"), DONE("done"), FAILED("failed"), UNKNOWN("");

    companion object {
        fun of(s: String?) = entries.firstOrNull { it.wire == s && it != UNKNOWN } ?: UNKNOWN
    }
}

enum class FailedAt {
    TRANSCRIBING, SUMMARISING;

    companion object { fun of(s: String?) = entries.firstOrNull { it.name.lowercase() == s } }
}

enum class Source {
    SIDECAR, LEGACY, CORRUPT;

    companion object { fun of(s: String?) = entries.firstOrNull { it.name.lowercase() == s } ?: SIDECAR }
}

/** One row of nativeListRecent. */
data class MeetingSummary(
    val id: String, val startedAt: String, val startedAtMs: Long, val durationMs: Long?,
    val status: Status, val failedAt: FailedAt?, val title: String?, val segmentCount: Int,
    val openTodoCount: Int, val source: Source, val folder: String,
    /** F16-FR5: "≈ $0.41", "≈ $0.41+" or "—", from the core. */
    val cost: String = "—",
) {
    companion object {
        fun from(o: JSONObject): MeetingSummary? { return MeetingSummary(
            id = o.str("id") ?: return null,
            startedAt = o.strOr("startedAt", ""),
            startedAtMs = o.long("startedAtMs") ?: 0,
            durationMs = o.long("durationMs"),
            status = Status.of(o.str("status")),
            failedAt = FailedAt.of(o.str("failedAt")),
            title = o.str("title"),
            segmentCount = o.int("segmentCount") ?: 0,
            openTodoCount = o.int("openTodoCount") ?: 0,
            source = Source.of(o.str("source")),
            folder = o.strOr("folder", ""),
            cost = Spend.label(o.obj("spend")),
        ) }
    }
}

/** One row of nativeOpenTodos. */
data class TodoRef(
    val meetingId: String, val meetingStartedAt: String, val meetingTitle: String?,
    val todoId: String, val text: String, val owner: String?, val due: String?,
) {
    companion object {
        fun from(o: JSONObject): TodoRef? { return TodoRef(
            meetingId = o.str("meetingId") ?: return null,
            meetingStartedAt = o.strOr("meetingStartedAt", ""),
            meetingTitle = o.str("meetingTitle"),
            todoId = o.str("todoId") ?: return null,
            text = o.strOr("text", ""),
            owner = o.str("owner"),
            due = o.str("due"),
        ) }
    }
}

/** [file] is null only in a hand-edited sidecar; the segment is kept so indexes still line up (N8). */
data class Segment(val file: String?, val startOffsetMs: Long, val durationMs: Long?, val text: String?, val flag: String?)
data class Todo(val id: String, val text: String, val owner: String?, val due: String?, val done: Boolean)
data class Section(val heading: String, val body: String)
data class MicRef(val kind: String, val name: String)
data class ProviderRun(val provider: String, val model: String)

/** The sidecar fields the shell shows. The shell never writes a sidecar; unknown fields stay in the core. */
data class Sidecar(
    val id: String, val startedAt: String, val durationMs: Long, val status: Status, val failedAt: FailedAt?,
    val title: String?, val titleSource: String?, val templateId: String?,
    val transcribeRun: ProviderRun?, val summariseRun: ProviderRun?,
    val summary: String?, val decisions: List<String>, val sections: List<Section>,
    val mic: MicRef?, val segments: List<Segment>, val error: String?, val note: String?, val todos: List<Todo>,
    /** F18-FR12: the linked event's key and title; null when unscheduled. */
    val eventKey: String? = null, val eventTitle: String? = null,
    /** F15: recorded by the near-live recorder (it has `live`, or chunks). */
    val nearLive: Boolean = false,
) {
    companion object {
        private fun run(o: JSONObject?): ProviderRun? =
            o?.let { ProviderRun(it.str("provider") ?: return@let null, it.strOr("model", "")) }

        fun from(o: JSONObject): Sidecar? { return Sidecar(
            id = o.str("id") ?: return null,
            startedAt = o.strOr("startedAt", ""),
            durationMs = o.long("durationMs") ?: 0,
            status = Status.of(o.str("status")),
            failedAt = FailedAt.of(o.str("failedAt")),
            title = o.str("title"), titleSource = o.str("titleSource"), templateId = o.str("templateId"),
            transcribeRun = run(o.obj("providers")?.obj("transcribe")),
            summariseRun = run(o.obj("providers")?.obj("summarise")),
            summary = o.str("summary"),
            decisions = o.arr("decisions").strings(),
            sections = o.arr("sections").mapObj { Section(it.strOr("heading", ""), it.strOr("body", "")) },
            mic = o.obj("mic")?.let { m -> MicRef(m.strOr("kind", ""), m.strOr("name", "")) },
            segments = o.arr("segments").mapObj {
                Segment(it.str("file"), it.long("startOffsetMs") ?: 0, it.long("durationMs"), it.str("text"), it.str("flag"))
            },
            error = o.str("error"), note = o.str("note"),
            todos = o.arr("todos").mapObj {
                Todo(it.str("id") ?: return@mapObj null, it.strOr("text", ""), it.str("owner"), it.str("due"), it.bool("done") ?: false)
            },
            eventKey = o.obj("event")?.str("eventKey"),
            eventTitle = o.obj("event")?.str("title"),
            nearLive = o.has("live") || (o.arr("chunks")?.length() ?: 0) > 0,
        ) }

        /** nativeTranscribe / nativeSummarise: `{meeting: <sidecar>}` once a job ran. */
        fun ofJob(o: JSONObject): Sidecar? = o.obj("meeting")?.let(::from)
    }
}

/** nativeGetMeeting. */
data class MeetingView(
    val meeting: Sidecar, val source: Source, val dir: String, val segmentPaths: List<String>, val transcriptPath: String?,
    /** F16-FR5: the meeting's cost so far, from the core. */
    val cost: String = "—",
    /** F15-FR6: a near-live meeting's transcript, one paragraph per chunk, as segments to page. */
    val paragraphs: List<Segment> = emptyList(),
) {
    /** What the transcript pages show: the chunks' paragraphs when there are any, else the segments. */
    val transcript: List<Segment> get() = paragraphs.ifEmpty { meeting.segments }

    companion object {
        fun from(o: JSONObject): MeetingView? { return MeetingView(
            meeting = o.obj("meeting")?.let(Sidecar::from) ?: return null,
            source = Source.of(o.str("source")),
            dir = o.strOr("dir", ""),
            segmentPaths = o.arr("segmentPaths").strings(),
            transcriptPath = o.str("transcriptPath"),
            cost = Spend.label(o.obj("spend")),
            paragraphs = o.arr("paragraphs").mapObj { Segment(null, it.long("atMs") ?: 0, null, it.str("text"), null) },
        ) }
    }
}

/** nativeBeginMeeting. */
data class Begun(val id: String, val firstSegmentPath: String, val maxDurationMs: Long, val maxFileBytes: Long) {
    companion object {
        fun from(o: JSONObject): Begun? {
            val plan = o.obj("plan") ?: return null
            return Begun(o.str("id") ?: return null, o.str("firstSegmentPath") ?: return null,
                plan.long("maxDurationMs") ?: return null, plan.long("maxFileBytes") ?: return null)
        }
    }
}

/** nativeAddSegment. */
data class SegmentAdded(val index: Int, val path: String) {
    companion object {
        fun from(o: JSONObject): SegmentAdded? { return SegmentAdded(o.int("index") ?: return null, o.str("path") ?: return null) }
    }
}

/** nativeRecover. `noAudio` is omitted by the core when empty. */
data class Recovery(val recovered: List<String>, val measure: List<ToMeasure>, val flagged: List<Flagged>, val noAudio: List<String>) {
    /** [chunks]: F15-FR8, the segment's finished chunk files, to rebuild it from if it can't be read. */
    data class ToMeasure(val id: String, val index: Int, val path: String, val chunks: List<Piece> = emptyList())
    /** A chunk file and how much of its start the previous chunk already holds. */
    data class Piece(val path: String, val skipMs: Long)
    data class Flagged(val id: String, val index: Int, val flag: String)

    companion object {
        fun from(o: JSONObject): Recovery = Recovery(
            o.arr("recovered").strings(),
            o.arr("measure").mapObj {
                ToMeasure(it.str("id") ?: return@mapObj null, it.int("index") ?: return@mapObj null, it.str("path") ?: return@mapObj null,
                    it.arr("chunks").mapObj { p -> Piece(p.str("path") ?: return@mapObj null, p.long("skipMs") ?: 0) })
            },
            o.arr("flagged").mapObj { Flagged(it.str("id") ?: return@mapObj null, it.int("index") ?: return@mapObj null, it.strOr("flag", "")) },
            o.arr("noAudio").strings(),
        )
    }
}
