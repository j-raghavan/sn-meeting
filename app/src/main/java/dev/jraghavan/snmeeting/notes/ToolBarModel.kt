package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.canvas.ShapeStyle

/**
 * F17-FR2 / F21-FR2: what the docked bar holds, top to bottom, for its state. Pure, so the
 * rules are JVM-tested: expanded it has Select, the ink tools, Shapes ▸, Insert ▸, Style ●,
 * Undo, Redo, Listen, ⋮ and ?; with a selection its middle is the selection's actions instead;
 * collapsed it is a thin strip with the current tool, the expand handle and the timer. **Mark and Stop (or Play
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
    /** F21-FR2: what opens beside the bar (one at a time; never over the canvas's far side). */
    enum class Panel { SHAPES, INSERT, STYLE, MORE, ERASER, TABLE }

    /** F21-FR2: the selection the bar's selection section acts on. [grouped]: it is one group
     *  (Ungroup is offered); [tableCell]: a table cell is the target (Table ▸ is offered). */
    data class Selection(val count: Int, val grouped: Boolean = false, val tableCell: Boolean = false)

    /** The selection section's actions (F21-FR2, FR4, FR5, FR13). */
    enum class Act { DELETE, DUPLICATE, GROUP, UNGROUP, FRONT, BACK, LINK, STYLE, TABLE, DONE }

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
        /** F21-FR2: the last shape used shows on the Shapes ▸ button. */
        val lastShape: NoteTool = NoteTool.RECTANGLE,
        /** F21-FR6: the default style, shown on Style ●. */
        val style: ShapeStyle = ShapeStyle.DEFAULT,
        val open: Panel? = null,
        val selection: Selection? = null,
    )

    sealed interface Item {
        /** A tool button; [enabled] false greys it (Listen while recording, F17-FR7). */
        data class Tool(val tool: NoteTool, val selected: Boolean, val enabled: Boolean) : Item
        data class Undo(val enabled: Boolean) : Item
        data class Redo(val enabled: Boolean) : Item
        /** F21-FR2: Shapes ▸, showing the last shape used. */
        data class Shapes(val shown: NoteTool, val selected: Boolean, val open: Boolean) : Item
        /** F21-FR2: Insert ▸ (text, sticky note, table, image). */
        data class Insert(val selected: Boolean, val open: Boolean) : Item
        /** F21-FR6: Style ●, the current colour and size. */
        data class Style(val style: ShapeStyle, val open: Boolean) : Item
        /** F21-FR12: ⋮ (zoom, Focus, refresh; Clear canvas, canvases, export as they land). */
        data class More(val open: Boolean) : Item
        /** F21-FR2: one of the selection section's actions. */
        data class Action(val act: Act, val enabled: Boolean = true) : Item
        /** F17-FR14: "?" shows the hints again. */
        data object Help : Item
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

    /** F21-FR2: the tools above the flyouts, top to bottom. */
    val TOOLS = listOf(NoteTool.SELECT, NoteTool.PEN, NoteTool.HIGHLIGHTER, NoteTool.ERASER, NoteTool.LASSO)
    const val MARK = "◆  Mark"

    /** The selection section (F21-FR2): replaces the tools while something is selected. */
    fun actions(sel: Selection): List<Item> = listOfNotNull(
        Item.Action(Act.DELETE),
        Item.Action(Act.DUPLICATE),
        if (sel.grouped) Item.Action(Act.UNGROUP) else Item.Action(Act.GROUP, enabled = sel.count >= 2),
        Item.Action(Act.FRONT),
        Item.Action(Act.BACK),
        Item.Action(Act.LINK),
        Item.Action(Act.STYLE),
        Item.Action(Act.TABLE).takeIf { sel.tableCell },
        Item.Action(Act.DONE),
    )

    fun items(s: State): List<Item> {
        val top = mutableListOf<Item>(Item.Handle(s.collapsed))
        val sel = s.selection?.takeIf { it.count > 0 }
        if (s.collapsed) {
            top += Item.Tool(s.tool, selected = true, enabled = true)
        } else if (sel != null) {
            top += actions(sel)
            top += Item.Undo(s.canUndo)
            top += Item.Redo(s.canRedo)
            top += Item.More(s.open == Panel.MORE)
            top += Item.Help
        } else {
            TOOLS.forEach { top += Item.Tool(it, it == s.tool, enabled = true) }
            top += Item.Shapes(if (s.tool.isShape) s.tool else s.lastShape, s.tool.isShape, s.open == Panel.SHAPES)
            top += Item.Insert(s.tool.isInsert, s.open == Panel.INSERT)
            top += Item.Style(s.style, s.open == Panel.STYLE)
            top += Item.Undo(s.canUndo)
            top += Item.Redo(s.canRedo)
            top += Item.Tool(NoteTool.LISTEN, s.tool == NoteTool.LISTEN, enabled = !s.recording)
            top += Item.More(s.open == Panel.MORE)
            top += Item.Help
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
