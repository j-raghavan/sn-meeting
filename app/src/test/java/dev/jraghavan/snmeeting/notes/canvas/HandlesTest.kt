package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.DrawEntry
import dev.jraghavan.snmeeting.notes.DrawStroke
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI

class HandlesTest { // F21-FR5: corners and a rotate handle, or a line's two ends
    private fun box(rotation: Double = 0.0, type: String = "rectangle", known: Boolean = true) =
        DrawItem("i:a", "a", type, known, 0.0, 0.0, 100.0, 50.0, rotation, null)

    @Test fun aBoxHasFourCornersAndARotateHandleAboveItsTop() {
        val hs = Handles.of(DrawEntry.Thing(box()), zoom = 2.0)
        assertEquals(listOf("tl", "tr", "bl", "br", "rotate"), hs.map { it.id })
        assertEquals(Handles.Handle(Handles.Kind.ROTATE, "rotate", 50.0, -28.0), hs[4])
        val turned = Handles.of(DrawEntry.Thing(box(PI / 2)), zoom = 1.0)
        assertEquals(75.0, turned[0].x, 1e-9)
        assertEquals(-25.0, turned[0].y, 1e-9)
        assertTrue(Handles.of(DrawEntry.Thing(box(known = false)), 1.0).isEmpty())
    }

    @Test fun linesHaveTheirEndsAndStrokesTheirBox() {
        val line = box(type = "arrow").copy(ends = listOf(1.0, 2.0, 3.0, 4.0))
        assertEquals(listOf("start", "end"), Handles.of(DrawEntry.Thing(line), 1.0).map { it.id })
        val st = DrawStroke(3, 0, 2f, 0, null, floatArrayOf(0f, 0f, 10f, 10f))
        assertEquals(5, Handles.of(DrawEntry.Ink(st), 1.0).size)
    }

    @Test fun thePenTakesTheNearestHandleRotateFirst() {
        val hs = Handles.of(DrawEntry.Thing(box()), zoom = 1.0)
        assertEquals("br", Handles.at(hs, 110.0, 60.0, 1.0)?.id)
        assertNull(Handles.at(hs, 50.0, 25.0, 1.0))
        assertEquals("rotate", Handles.at(hs, 50.0, -50.0, 1.0)?.id)
        assertEquals(150.0, Handles.resizedWidth(box(), "br", 150.0, 80.0), 1e-9)
        assertEquals(30.0, Handles.resizedWidth(box(), "tl", 70.0, 0.0), 1e-9)
    }
}
