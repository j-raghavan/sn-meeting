package dev.jraghavan.snmeeting.notes

/**
 * F17-FR4 (device finding 2026-10-09: ink "disappeared" while writing): when the canvas's own
 * rendering takes over from the firmware's pen overlay, as inkread does it
 * (`StylusInkController.commitStroke`, `ReaderActivity.repaintPanel` / `clearFirmwareInk`).
 *
 * A pen stroke rides the firmware overlay and is never cleared or redrawn after it ends: it is
 * only committed to the core. The canvas "bakes" (draws every committed stroke itself, asks the
 * panel for a refresh, and only then clears the overlay; never clear first) when the overlay no
 * longer matches the page: the viewport changed, an edit removed or moved ink, a tool whose
 * overlay is off took over, the page goes, or the user asked for a refresh. The highlighter's
 * overlay is off (it draws its own band), so its stroke bakes at once, as inkread's does.
 */
object InkBake {
    enum class Event {
        PEN_STROKE, HIGHLIGHTER_STROKE, ERASE, UNDO_REDO, LASSO_MOVE,
        VIEWPORT_SETTLED, TOOL_CHANGED, PAGE_LEAVING, USER_REFRESH,
    }

    enum class Step { DRAW, REFRESH_PANEL, CLEAR_OVERLAY }

    /** The events that change the ink itself (not just what is in view): F17-FR13's thumbnail
     *  is rebuilt after these. */
    val INK_CHANGES = setOf(Event.PEN_STROKE, Event.HIGHLIGHTER_STROKE, Event.ERASE, Event.UNDO_REDO, Event.LASSO_MOVE)

    private val BAKE = listOf(Step.DRAW, Step.REFRESH_PANEL, Step.CLEAR_OVERLAY)

    /** What the canvas does for [e]; [toolInks] is whether the tool now selected writes with
     *  the firmware overlay (the pen only). */
    fun plan(e: Event, toolInks: Boolean = true): List<Step> = when (e) {
        Event.PEN_STROKE -> emptyList()
        Event.TOOL_CHANGED -> if (toolInks) emptyList() else BAKE
        Event.PAGE_LEAVING -> listOf(Step.CLEAR_OVERLAY) // the page's own rendering is saved; the overlay must not leak
        else -> BAKE
    }

    /** Only the pen's ink rides the firmware overlay (inkread `applyToolInkState`). */
    fun overlayFor(tool: NoteTool): Boolean = tool == NoteTool.PEN
}
