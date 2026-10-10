package dev.jraghavan.snmeeting.notes

/**
 * F17-FR2: what the docked bar holds, top to bottom, for its state. Pure, so the rules are
 * JVM-tested: expanded it has every tool, Undo, Redo, the options and Listen; collapsed it is a
 * thin strip with the current tool, the expand handle and the timer. **Mark and Stop (or Play
 * and the position when not recording) are at the bottom in both states.**
 */
/**
 * F17-FR1's top bar, right side (owner, 2026-10-09): only the recording timer, "● 12:41",
 * while this meeting records; empty otherwise (the date and length are the Minutes header's).
 */
object TopBar {
    fun right(recordingMs: Long?): String =
        recordingMs?.let { "●  " + dev.jraghavan.snmeeting.model.CalendarFormat.length(it) } ?: ""
}

object ToolBarModel {
    data class State(
        val tool: NoteTool,
        val collapsed: Boolean,
        val recording: Boolean,
        val canUndo: Boolean = false,
        val canRedo: Boolean = false,
        /** "0:12:41" while recording; the position when not. */
        val timer: String = "",
        /** F16-FR5: "≈ $0.08 so far" (near-live meetings), or null. */
        val cost: String? = null,
        val playing: Boolean = false,
        val canPlay: Boolean = false,
        val focus: Boolean = false,
        /** "Marked 0:12:41" for 2 s after a Mark (F17-FR3), else null. */
        val marked: String? = null,
    )

    sealed interface Item {
        /** A tool button; [enabled] false greys it (Listen while recording, F17-FR7). */
        data class Tool(val tool: NoteTool, val selected: Boolean, val enabled: Boolean) : Item
        data class Undo(val enabled: Boolean) : Item
        data class Redo(val enabled: Boolean) : Item
        /** Thickness, Focus mode and a full refresh (F17-FR6, FR11). */
        data class Options(val focus: Boolean) : Item
        /** Collapse (expanded) or expand (collapsed). */
        data class Handle(val collapsed: Boolean) : Item
        data class Timer(val text: String) : Item
        data class Cost(val text: String) : Item
        data class Mark(val label: String) : Item
        data object Stop : Item
        data class Play(val playing: Boolean, val enabled: Boolean) : Item
        /** The spacer that pushes the bottom group down. */
        data object Gap : Item
    }

    val TOOLS = listOf(NoteTool.PEN, NoteTool.HIGHLIGHTER, NoteTool.ERASER, NoteTool.LASSO)
    const val MARK = "◆  Mark"

    fun items(s: State): List<Item> {
        val top = mutableListOf<Item>(Item.Handle(s.collapsed))
        if (s.collapsed) {
            top += Item.Tool(s.tool, selected = true, enabled = true)
        } else {
            TOOLS.forEach { top += Item.Tool(it, it == s.tool, enabled = true) }
            top += Item.Undo(s.canUndo)
            top += Item.Redo(s.canRedo)
            top += Item.Tool(NoteTool.LISTEN, s.tool == NoteTool.LISTEN, enabled = !s.recording)
            top += Item.Options(s.focus)
        }
        top += Item.Gap
        val bottom = if (s.recording) {
            listOfNotNull(Item.Timer(s.timer), s.cost?.takeIf { !s.collapsed }?.let { Item.Cost(it) }, Item.Mark(s.marked ?: MARK), Item.Stop)
        } else {
            listOf(Item.Timer(s.timer), Item.Play(s.playing, s.canPlay))
        }
        return top + bottom
    }
}
