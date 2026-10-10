package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.InkBake.Event
import dev.jraghavan.snmeeting.notes.InkBake.Step
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InkBakeTest { // F17-FR4, as inkread keeps pen ink on the firmware overlay
    @Test fun aPenStrokeIsNeverClearedOrRedrawn() {
        assertTrue(InkBake.plan(Event.PEN_STROKE).isEmpty())
    }

    @Test fun everythingElseBakesDrawingAndRefreshingBeforeTheOverlayClears() {
        for (e in listOf(Event.HIGHLIGHTER_STROKE, Event.ERASE, Event.UNDO_REDO, Event.LASSO_MOVE,
                Event.VIEWPORT_SETTLED, Event.USER_REFRESH)) {
            val p = InkBake.plan(e)
            assertEquals("$e", listOf(Step.DRAW, Step.REFRESH_PANEL, Step.CLEAR_OVERLAY), p)
            assertTrue("$e: the clear comes last", p.indexOf(Step.CLEAR_OVERLAY) > p.indexOf(Step.REFRESH_PANEL))
        }
    }

    @Test fun aToolChangeBakesOnlyWhenTheOverlayGoesOff() {
        assertTrue(InkBake.plan(Event.TOOL_CHANGED, toolInks = true).isEmpty())
        assertEquals(Step.CLEAR_OVERLAY, InkBake.plan(Event.TOOL_CHANGED, toolInks = false).last())
        assertEquals(listOf(Step.CLEAR_OVERLAY), InkBake.plan(Event.PAGE_LEAVING))
    }

    @Test fun onlyThePenWritesWithTheOverlay() {
        assertTrue(InkBake.overlayFor(NoteTool.PEN))
        for (t in listOf(NoteTool.HIGHLIGHTER, NoteTool.ERASER, NoteTool.LASSO, NoteTool.LISTEN)) assertFalse("$t", InkBake.overlayFor(t))
    }
}
