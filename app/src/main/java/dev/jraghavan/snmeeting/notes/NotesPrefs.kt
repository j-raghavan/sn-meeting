package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.bool
import dev.jraghavan.snmeeting.model.obj
import dev.jraghavan.snmeeting.model.str
import org.json.JSONObject

/**
 * `settings.json` → `notes` (F17-FR2, Data Model): where the tool bar docks (right or left;
 * never top, OQ13), whether it is collapsed, Focus mode, and "Use my notes for the minutes"
 * (on by default, F17-FR9 / OQ20). They apply to every meeting and survive restarts.
 */
data class NotesPrefs(
    val dock: Dock = Dock.RIGHT,
    val collapsed: Boolean = false,
    val focus: Boolean = false,
    val useForMinutes: Boolean = true,
    /** OQ20: the one-time "notes go to <provider>" notice, answered per provider. */
    val noticeShown: Map<String, Boolean> = emptyMap(),
) {
    enum class Dock(val wire: String) { RIGHT("right"), LEFT("left") }

    companion object {
        fun from(o: JSONObject?): NotesPrefs = NotesPrefs(
            dock = if (o?.str("dock") == Dock.LEFT.wire) Dock.LEFT else Dock.RIGHT,
            collapsed = o?.str("sidebar") == "collapsed",
            focus = o?.bool("focus") ?: false,
            useForMinutes = o?.bool("useForMinutes") ?: true,
            noticeShown = listOf("openai", "gemini", "anthropic").associateWith { o?.obj("noticeShown")?.bool(it) ?: false },
        )
    }
}

/**
 * OQ20 / F17-FR9 (Security): the one-time notice, the first time notes would go to the
 * summarise [provider]: null when the setting is off or it was answered.
 */
object NotesNotice {
    const val TITLE = "Your notes and the minutes"

    fun of(p: NotesPrefs, provider: String?, name: String): String? {
        if (provider == null || !p.useForMinutes || p.noticeShown[provider] == true) return null
        return "When you stop, images of these notes go to $name with the transcript, so the minutes " +
            "and Search can use your handwriting. Each provider may keep what it receives under its own " +
            "policy. Turn this off in Settings → Notes."
    }
}

/** RFC 7396 patches for `notes` (shape tested on the JVM). */
object NotesPatch {
    fun dock(d: NotesPrefs.Dock) = """{"notes":{"dock":${CoreJson.quote(d.wire)}}}"""
    fun collapsed(c: Boolean) = """{"notes":{"sidebar":"${if (c) "collapsed" else "expanded"}"}}"""
    fun focus(on: Boolean) = """{"notes":{"focus":$on}}"""
    fun useForMinutes(on: Boolean) = """{"notes":{"useForMinutes":$on}}"""
    fun noticeShown(provider: String) = """{"notes":{"noticeShown":{${CoreJson.quote(provider)}:true}}}"""
}

/**
 * F17-FR2's geometry: the bar sits on the dock side and the canvas starts beside it, never
 * under it. Collapsing narrows the bar to an edge strip and widens the canvas, as one change.
 */
object DockLayout {
    /** `[left, right)` of the bar and of the canvas across a page [width] wide. */
    data class Spans(val barLeft: Int, val barRight: Int, val canvasLeft: Int, val canvasRight: Int) {
        val canvasWidth get() = canvasRight - canvasLeft
    }

    fun spans(width: Int, dock: NotesPrefs.Dock, collapsed: Boolean, expandedPx: Int, collapsedPx: Int): Spans {
        val bar = (if (collapsed) collapsedPx else expandedPx).coerceIn(0, width)
        return when (dock) {
            NotesPrefs.Dock.RIGHT -> Spans(width - bar, width, 0, width - bar)
            NotesPrefs.Dock.LEFT -> Spans(0, bar, bar, width)
        }
    }
}
