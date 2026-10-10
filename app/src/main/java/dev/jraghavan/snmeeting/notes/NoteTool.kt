package dev.jraghavan.snmeeting.notes

/**
 * The notes tools (F13-FR1b, F17-FR2/FR7), selected explicitly as in inkread's tool model
 * (ADR-INKREAD-0010, `ToolPalette.kt:20-42`, AGPL-3.0, same owner), with inkread's book-only
 * Define removed and Listen added. [core] is the `nativeInkStroke` tool code (ink tools only).
 */
enum class NoteTool(val label: String, val core: Int) {
    PEN("Pen", 0),
    HIGHLIGHTER("Highlighter", 1),
    ERASER("Eraser", -1),
    LASSO("Lasso", -1),
    LISTEN("Listen", -1),
    ;

    val inks get() = core >= 0

    companion object {
        /**
         * Which tool a pen event drives: the selected one, except an inverted pen
         * (`TOOL_TYPE_ERASER`), which always erases (F13-FR4; inkread #158).
         */
        fun forPen(selected: NoteTool, inverted: Boolean): NoteTool = if (inverted) ERASER else selected

        fun of(name: String?): NoteTool = entries.firstOrNull { it.name == name } ?: PEN
    }
}

/** What a contact on the canvas does (F17-FR4, F17-FR7, INV12b). */
enum class Gesture {
    /** Pen ink with the selected ink tool. */
    DRAW,
    ERASE,
    LASSO,
    /** A tap that seeks to the ink under it (Listen only). */
    LISTEN,
    /** A finger drag pans; a pinch zooms. */
    PAN,
    /** Nothing: a palm, or a finger tap with any tool but Listen. */
    NONE,
}

/**
 * F17-FR4 / FR7 (OQ19): the pen writes with the selected tool and an inverted pen erases; a
 * finger never draws: a drag pans, and a tap does nothing to the ink unless Listen is
 * selected. While recording, Listen is disabled, so it then acts as nothing at all.
 */
object InputRules {
    fun forPen(selected: NoteTool, inverted: Boolean, recording: Boolean): Gesture =
        when (NoteTool.forPen(selected, inverted)) {
            NoteTool.PEN, NoteTool.HIGHLIGHTER -> Gesture.DRAW
            NoteTool.ERASER -> Gesture.ERASE
            NoteTool.LASSO -> Gesture.LASSO
            NoteTool.LISTEN -> if (recording) Gesture.NONE else Gesture.LISTEN
        }

    /** A finger that moved past the slop pans; one that didn't is a tap. A palm does nothing. */
    fun forFinger(selected: NoteTool, isTap: Boolean, palm: Boolean, recording: Boolean): Gesture = when {
        palm -> Gesture.NONE
        !isTap -> Gesture.PAN
        selected == NoteTool.LISTEN && !recording -> Gesture.LISTEN
        else -> Gesture.NONE
    }
}
