package dev.jraghavan.snmeeting.notes

/**
 * The notes tools (F13-FR1b, F17-FR2/FR7, F21-FR2), selected explicitly as in inkread's tool
 * model (ADR-INKREAD-0010, `ToolPalette.kt:20-42`, AGPL-3.0, same owner), with inkread's
 * book-only Define removed and Listen added; F21 adds sn-canvas's Select, shapes, text, sticky
 * notes and tables. [core] is the `nativeInkStroke` tool code (ink tools only).
 */
enum class NoteTool(val label: String, val core: Int) {
    SELECT("Select", -1),
    PEN("Pen", 0),
    HIGHLIGHTER("Highlighter", 1),
    ERASER("Eraser", -1),
    LASSO("Lasso", -1),
    LISTEN("Listen", -1),
    RECTANGLE("Rectangle", -1),
    ELLIPSE("Ellipse", -1),
    LINE("Line", -1),
    ARROW("Arrow", -1),
    TEXT("Text", -1),
    NOTE("Sticky note", -1),
    TABLE("Table", -1),
    ;

    val inks get() = core >= 0

    /** One of the Shapes ▸ flyout's tools (F21-FR2). */
    val isShape get() = this in SHAPES

    /** One of the Insert ▸ flyout's tools (Image there is an action, not a tool). */
    val isInsert get() = this in INSERTS

    companion object {
        val SHAPES = listOf(RECTANGLE, ELLIPSE, LINE, ARROW)
        val INSERTS = listOf(TEXT, NOTE, TABLE)

        /**
         * Which tool a pen event drives: the selected one, except an inverted pen
         * (`TOOL_TYPE_ERASER`), which always erases, strokes and items alike (F13-FR4,
         * F21-FR2; inkread #158).
         */
        fun forPen(selected: NoteTool, inverted: Boolean): NoteTool = if (inverted) ERASER else selected

        fun of(name: String?): NoteTool = entries.firstOrNull { it.name == name } ?: PEN
    }
}

/** What a contact on the canvas does (F17-FR4, F17-FR7, INV12b, F21-FR2). */
enum class Gesture {
    /** Pen ink with the selected ink tool. */
    DRAW,
    ERASE,
    LASSO,
    /** A tap that seeks to the ink under it (Listen only). */
    LISTEN,
    /** F21-FR4: the Select tool's pen: a tap selects, a drag on empty canvas draws a selection
     *  rectangle, a drag on the selection moves it. */
    SELECT,
    /** F21-FR8: a pen drag draws a rectangle, ellipse, line or arrow. */
    SHAPE,
    /** F21-FR9: a pen tap places a text box or sticky note. */
    PLACE,
    /** F21-FR10: a pen drag outlines a table. */
    TABLE,
    /** A finger drag pans; a pinch zooms. */
    PAN,
    /** Nothing: a palm, or a finger tap with any tool but Listen. */
    NONE,
}

/**
 * F17-FR4 / FR7 (OQ19), F21-FR2 (INV21): the pen acts with the selected tool and an inverted
 * pen erases; a finger never draws, selects, moves or erases: a drag pans, and a tap does
 * nothing unless Listen is selected (or it lands on a link badge, F21-FR13, which the canvas
 * checks first). While recording, Listen is disabled, so it then acts as nothing at all.
 */
object InputRules {
    fun forPen(selected: NoteTool, inverted: Boolean, recording: Boolean): Gesture =
        when (NoteTool.forPen(selected, inverted)) {
            NoteTool.PEN, NoteTool.HIGHLIGHTER -> Gesture.DRAW
            NoteTool.ERASER -> Gesture.ERASE
            NoteTool.LASSO -> Gesture.LASSO
            NoteTool.LISTEN -> if (recording) Gesture.NONE else Gesture.LISTEN
            NoteTool.SELECT -> Gesture.SELECT
            NoteTool.RECTANGLE, NoteTool.ELLIPSE, NoteTool.LINE, NoteTool.ARROW -> Gesture.SHAPE
            NoteTool.TEXT, NoteTool.NOTE -> Gesture.PLACE
            NoteTool.TABLE -> Gesture.TABLE
        }

    /** A finger that moved past the slop pans; one that didn't is a tap. A palm does nothing. */
    fun forFinger(selected: NoteTool, isTap: Boolean, palm: Boolean, recording: Boolean): Gesture = when {
        palm -> Gesture.NONE
        !isTap -> Gesture.PAN
        selected == NoteTool.LISTEN && !recording -> Gesture.LISTEN
        else -> Gesture.NONE
    }
}
