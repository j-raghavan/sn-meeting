package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** sn-canvas's arbitration (pen over fingers, pinch over one finger) and OQ19's input rules. */
class TouchArbiterTest {
    private val finger = Contact(id = 0, isPen = false, x = 10f, y = 20f)
    private val second = Contact(id = 1, isPen = false, x = 300f, y = 400f)
    private val pen = Contact(id = 2, isPen = true, x = 50f, y = 60f)

    @Test fun aFingerStartsMovesAndEnds() {
        val a = TouchArbiter()
        assertNull(a.move(listOf(finger)))
        assertEquals(listOf(PointerInput.Start(finger)), a.down(finger))
        assertEquals(0, a.followedId)
        assertNull(a.move(listOf(second)))
        assertEquals(PointerInput.Move(15f, 25f), a.move(listOf(finger.copy(x = 15f, y = 25f))))
        assertNull(a.up(second, isLastContact = false))
        assertEquals(PointerInput.End(16f, 26f), a.up(finger.copy(x = 16f, y = 26f), isLastContact = true))
        assertFalse(a.isPenActive)
    }

    @Test fun aSecondFingerPinchesUntilEveryFingerLifts() {
        val a = TouchArbiter()
        a.down(finger)
        assertEquals(listOf(PointerInput.Abandon), a.down(second))
        assertEquals(emptyList<PointerInput>(), a.down(finger.copy(id = 3)))
        assertNull(a.up(second, isLastContact = false))
        assertNull(a.up(finger, isLastContact = true))
        assertEquals(listOf(PointerInput.Start(finger)), a.down(finger))
    }

    @Test fun thePenTakesOverFromAPalmAndFingersWaitWhileItWrites() {
        val a = TouchArbiter()
        a.down(finger)
        assertEquals(listOf(PointerInput.Abandon, PointerInput.Start(pen)), a.down(pen))
        assertTrue(a.isPenActive)
        assertEquals(emptyList<PointerInput>(), a.down(second))
        assertEquals(emptyList<PointerInput>(), a.down(pen.copy(id = 4)))
        assertEquals(PointerInput.Move(55f, 65f, 1f), a.move(listOf(finger, pen.copy(x = 55f, y = 65f))))
        assertEquals(PointerInput.Abandon, a.cancel())
        assertNull(a.cancel())
        assertEquals(listOf(PointerInput.Start(pen)), a.down(pen))
    }

    @Test fun thePenWritesAndAnInvertedPenErases() { // F17-FR4, F13-FR4, F17-AC9
        assertEquals(Gesture.DRAW, InputRules.forPen(NoteTool.PEN, inverted = false, recording = true))
        assertEquals(Gesture.DRAW, InputRules.forPen(NoteTool.HIGHLIGHTER, inverted = false, recording = false))
        assertEquals(Gesture.ERASE, InputRules.forPen(NoteTool.PEN, inverted = true, recording = true))
        assertEquals(Gesture.ERASE, InputRules.forPen(NoteTool.LISTEN, inverted = true, recording = false))
        assertEquals(Gesture.ERASE, InputRules.forPen(NoteTool.ERASER, inverted = false, recording = false))
        assertEquals(Gesture.LASSO, InputRules.forPen(NoteTool.LASSO, inverted = false, recording = false))
        assertEquals(Gesture.LISTEN, InputRules.forPen(NoteTool.LISTEN, inverted = false, recording = false))
        assertEquals(Gesture.NONE, InputRules.forPen(NoteTool.LISTEN, inverted = false, recording = true))
    }

    @Test fun theNewToolsActWithThePenAndTheInvertedPenStillErases() { // F21-FR2, F21-AC6
        assertEquals(Gesture.SELECT, InputRules.forPen(NoteTool.SELECT, inverted = false, recording = false))
        for (t in NoteTool.SHAPES) assertEquals(Gesture.SHAPE, InputRules.forPen(t, inverted = false, recording = true))
        assertEquals(Gesture.PLACE, InputRules.forPen(NoteTool.TEXT, inverted = false, recording = false))
        assertEquals(Gesture.PLACE, InputRules.forPen(NoteTool.NOTE, inverted = false, recording = false))
        assertEquals(Gesture.TABLE, InputRules.forPen(NoteTool.TABLE, inverted = false, recording = false))
        for (t in NoteTool.entries) assertEquals("$t", Gesture.ERASE, InputRules.forPen(t, inverted = true, recording = false))
        assertTrue(NoteTool.ARROW.isShape && !NoteTool.TEXT.isShape && NoteTool.TABLE.isInsert && !NoteTool.SELECT.isInsert)
    }

    @Test fun aFingerNeverDrawsAndATapSeeksOnlyWithListen() { // INV12b, F17-AC1c
        for (t in NoteTool.entries) {
            assertEquals(Gesture.PAN, InputRules.forFinger(t, isTap = false, palm = false, recording = false))
            assertEquals(Gesture.NONE, InputRules.forFinger(t, isTap = false, palm = true, recording = false))
            val tap = InputRules.forFinger(t, isTap = true, palm = false, recording = false)
            assertEquals(if (t == NoteTool.LISTEN) Gesture.LISTEN else Gesture.NONE, tap)
            assertEquals(Gesture.NONE, InputRules.forFinger(t, isTap = true, palm = false, recording = true))
        }
    }

    @Test fun aPalmIsTheWritingHand() {
        assertTrue(PalmRule.isPalm(penHovering = true, penDown = false, msSincePen = 99_999, touchMajorPx = 1f, viewHeightPx = 1872))
        assertTrue(PalmRule.isPalm(false, penDown = true, msSincePen = 99_999, touchMajorPx = 1f, viewHeightPx = 1872))
        assertTrue(PalmRule.isPalm(false, false, msSincePen = 400, touchMajorPx = 1f, viewHeightPx = 1872))
        assertTrue(PalmRule.isPalm(false, false, 99_999, touchMajorPx = 150f, viewHeightPx = 1872))
        assertFalse(PalmRule.isPalm(false, false, 99_999, touchMajorPx = 40f, viewHeightPx = 1872))
        assertFalse(PalmRule.isPalm(false, false, 99_999, touchMajorPx = 400f, viewHeightPx = 0))
    }

    @Test fun toolsByName() {
        assertEquals(NoteTool.LASSO, NoteTool.of("LASSO"))
        assertEquals(NoteTool.PEN, NoteTool.of(null))
        assertEquals(NoteTool.PEN, NoteTool.of("DEFINE"))
        assertTrue(NoteTool.PEN.inks && NoteTool.HIGHLIGHTER.inks && !NoteTool.LISTEN.inks)
    }
}
