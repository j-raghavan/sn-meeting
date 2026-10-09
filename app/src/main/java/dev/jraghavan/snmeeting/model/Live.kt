package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** One whole line of the live transcript, at its estimated recording-clock time (F15-FR6). */
data class LiveLine(val n: Int, val atMs: Long, val text: String)

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
            lines = o.arr("lines").mapObj { LiveLine(it.int("n") ?: return@mapObj null, it.long("atMs") ?: 0, it.strOr("text", "")) },
            waiting = o.int("waiting") ?: 0,
            state = o.strOr("state", OFF),
            reason = o.str("reason"),
            spent = o.strOr("spent", "—"),
        )
    }
}
