package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** F15-FR11: a timestamped marker on the recording clock, perhaps labelled. */
data class Mark(val id: String, val atMs: Long, val label: String?) {
    /** `◆ [0:12:41] Mark`, or its label. */
    val line: String get() = "◆ [${Format.duration(atMs)}] ${label ?: "Mark"}"

    companion object {
        fun from(o: JSONObject): Mark? {
            return Mark(o.str("id") ?: return null, o.long("atMs") ?: return null, o.str("label"))
        }
    }
}

/** One whole line of the live transcript, at its estimated recording-clock time (F15-FR6);
 *  [markId] is set on a mark's own line (F15-FR11, F17-FR6). */
data class LiveLine(val n: Int, val atMs: Long, val text: String, val markId: String? = null)

/**
 * `nativeLiveTranscript` (F15-FR6 / FR7, F17-FR6): the lines so far and where the lane is.
 * [spent] is the meeting's cost so far ("≈ $0.08", F16-FR5).
 */
data class LiveTranscript(
    val lines: List<LiveLine>, val waiting: Int, val state: String, val reason: String?, val spent: String,
) {
    /** The strip's right end: "Offline · 4 waiting", "Transcribing…", the pause's reason, or nothing. */
    val status: String get() = when (state) {
        OFFLINE -> "Offline · $waiting waiting"
        RETRYING -> "Retrying · $waiting waiting"
        TRANSCRIBING -> "Transcribing…"
        PAUSED -> reason ?: "Transcription paused"
        else -> ""
    }

    /** F16-FR5: the recording toolbar's "≈ $0.08 so far" (the sum of this meeting's records). */
    val soFar: String get() = if (spent == "—") "Nothing spent yet" else "$spent so far"

    /** The strip: the newest [STRIP_LINES] lines, or every line when opened. */
    fun shown(all: Boolean): List<LiveLine> = if (all) lines else lines.takeLast(STRIP_LINES)

    companion object {
        const val OFF = "off"
        const val PAUSED = "paused"
        const val OFFLINE = "offline"
        /** The provider was busy (429, 5xx); the lane backs off as when offline. */
        const val RETRYING = "retrying"
        const val TRANSCRIBING = "transcribing"
        const val STRIP_LINES = 3
        /** Shown before the first line arrives. */
        const val EMPTY = "The transcript fills in about twice a minute."

        fun from(o: JSONObject): LiveTranscript = LiveTranscript(
            lines = o.arr("lines").mapObj { LiveLine(it.int("n") ?: return@mapObj null, it.long("atMs") ?: 0, it.strOr("text", ""), it.str("mark")) },
            waiting = o.int("waiting") ?: 0,
            state = o.strOr("state", OFF),
            reason = o.str("reason"),
            spent = o.strOr("spent", "—"),
        )
    }
}
