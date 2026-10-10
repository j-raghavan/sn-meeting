package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.DrawEntry
import dev.jraghavan.snmeeting.notes.DrawStroke
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BadgesTest { // F21-FR13: a chain badge at the top-right corner
    private val link = DrawLink("meeting", "meeting-20261008-090000", true)
    private val box = DrawItem("i:a", "a", "rectangle", true, 0.0, 0.0, 100.0, 50.0, 0.0, null, link = link)

    @Test fun theBadgeSitsOffTheTopRightCorner() {
        assertEquals(Pt(134.0, -34.0), Badges.point(DrawEntry.Thing(box), 1.0))
        assertEquals(Pt(117.0, -17.0), Badges.point(DrawEntry.Thing(box), 2.0))
        val arrow = box.copy(type = "arrow", ends = listOf(0.0, 0.0, 300.0, 40.0))
        assertEquals(Pt(300.0, 40.0), Badges.point(DrawEntry.Thing(arrow), 1.0))
        val st = DrawStroke(1, 0, 2f, 0, null, floatArrayOf(0f, 0f, 10f, 10f), linked = true)
        assertEquals(Pt(11.0 + 34.0, -1.0 - 34.0), Badges.point(DrawEntry.Ink(st), 1.0))
        val ellipse = Badges.point(DrawEntry.Thing(box.copy(type = "ellipse")), 1.0)
        assertEquals(50.0 + 50.0 * (1 + Math.sqrt(2.0) / 2) - 50.0 + 34.0, ellipse.x, 1e-9)
    }

    @Test fun aTapTakesTheTopmostBadge() {
        val entries = listOf(DrawEntry.Thing(box), DrawEntry.Thing(box.copy(ref = "i:b", id = "b")),
            DrawEntry.Thing(box.copy(ref = "i:c", id = "c", link = null)))
        assertEquals("i:b", Badges.at(entries, 134.0, -30.0, 1.0, 24.0)?.ref)
        assertNull(Badges.at(entries, 50.0, 25.0, 1.0, 24.0))
    }
}
