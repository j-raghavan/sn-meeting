package dev.jraghavan.snmeeting.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ViewportTest { // F13-FR1 / F17-FR1: pan, pinch-zoom 5 %–2000 %, the zoom pill, Fit, minimap
    private val eps = 1e-9

    @Test fun screenAndWorldAreInverse() {
        val v = Viewport(-500.0, 1_000.0, 2.0)
        assertEquals(200.0, v.screenX(-400.0), eps)
        assertEquals(-400.0, v.worldX(200.0), eps)
        assertEquals(1_050.0, v.worldY(100.0), eps)
        assertEquals(100.0, v.screenY(1_050.0), eps)
        assertEquals(WorldRect(-500.0, 1_000.0, 0.0, 1_400.0), v.visible(1_000.0, 800.0))
    }

    @Test fun aDragMovesTheWorldWithTheFinger() {
        val v = Viewport(0.0, 0.0, 0.5).panBy(100.0, -50.0)
        assertEquals(-200.0, v.x, eps)
        assertEquals(100.0, v.y, eps)
    }

    @Test fun aPinchKeepsItsFocusAndStaysInRange() {
        val v = Viewport(0.0, 0.0, 1.0)
        val z = v.zoomAbout(2.0, 300.0, 200.0)
        assertEquals(2.0, z.zoom, eps)
        assertEquals(v.worldX(300.0), z.worldX(300.0), eps)
        assertEquals(v.worldY(200.0), z.worldY(200.0), eps)
        assertEquals(Viewport.MAX_ZOOM, v.zoomAbout(1_000.0, 0.0, 0.0).zoom, eps)
        assertEquals(Viewport.MIN_ZOOM, v.zoomAbout(0.0001, 0.0, 0.0).zoom, eps)
        assertEquals(v, v.zoomAbout(0.0, 1.0, 1.0))
        assertEquals(v, v.zoomAbout(Double.NaN, 1.0, 1.0))
    }

    @Test fun thePillsPresetsZoomAboutTheCentre() {
        val v = Viewport.centred(500.0, 500.0, 1.0, 1_000.0, 800.0)
        val half = v.zoomToPercent(50, 1_000.0, 800.0)
        assertEquals(0.5, half.zoom, eps)
        val (cx, cy) = half.centre(1_000.0, 800.0)
        assertEquals(500.0, cx, eps)
        assertEquals(500.0, cy, eps)
        assertEquals(listOf(100, 50, 25), Viewport.PRESETS)
        assertEquals(Viewport.MAX_ZOOM, Viewport.centred(0.0, 0.0, 99.0, 10.0, 10.0).zoom, eps)
    }

    @Test fun fitBringsFarInkBackButNeverMagnifies() { // EC21
        val far = WorldRect(40_000.0, -9_000.0, 44_000.0, -7_000.0)
        val v = Viewport.fitInk(far, 1_000.0, 800.0, 40.0)
        assertTrue(v.zoom < 1.0)
        val vis = v.visible(1_000.0, 800.0)
        assertTrue(vis.left <= far.left && vis.right >= far.right && vis.top <= far.top && vis.bottom >= far.bottom)
        val small = Viewport.fitInk(WorldRect(0.0, 0.0, 10.0, 0.0), 1_000.0, 800.0, 40.0)
        assertEquals(1.0, small.zoom, eps)
        assertEquals(Viewport(0.0, 0.0, 1.0), Viewport.fitInk(null, 1_000.0, 800.0, 40.0))
        assertEquals(WorldRect(0.0, 0.0, 2.0, 3.0), WorldRect.of(listOf(0.0, 0.0, 2.0, 3.0)))
        assertNull(WorldRect.of(listOf(1.0)))
        assertNull(WorldRect.of(null))
    }

    @Test fun theMinimapCardSitsAtTheTopCornerBesideTheBar() { // F17-FR13; owner 2026-10-10: only the minimap moves to the top right
        val ink = WorldRect(0.0, 0.0, 5_000.0, 5_000.0)
        val view = Viewport(10_000.0, 10_000.0, 1.0)
        val d = 2.0
        // A fifth of a wide canvas; never under 160 dp on a narrow one.
        val right = MinimapFrame.of(ink, view, 2_000.0, 2_400.0, barOnLeft = false, d, top = 100.0)!!
        assertEquals(400.0, right.width, eps)
        assertEquals(480.0, right.height, eps) // the canvas's shape
        assertEquals(320.0, MinimapFrame.of(ink, view, 1_200.0, 1_400.0, false, d)!!.width, eps)
        assertEquals(2_000.0 - (12.0 + 6.0) * d, right.left + right.width, eps) // the bar is on the right
        assertEquals(100.0 + 6.0 * d, right.top, eps) // under the zoom pill
        val left = MinimapFrame.of(ink, view, 2_000.0, 2_400.0, barOnLeft = true, d)!!
        assertEquals((12.0 + 6.0) * d, left.left, eps)
        assertEquals((12.0 + 6.0) * d, left.top, eps)
        // The card wraps the thumbnail and the −/+ row.
        assertEquals(right.left - 12.0, right.cardLeft, eps) // pad 6 dp at 2 px/dp
        assertEquals(right.top + right.height + 6.0 * d, right.buttonTop, eps)
        assertEquals(right.buttonTop + 48.0 * d + 12.0, right.cardBottom, eps)
        assertNull(MinimapFrame.of(ink, view, 0.0, 10.0, false))
        assertTrue(ink.contains(1.0, 1.0))
        assertFalse(ink.contains(-1.0, 1.0))
    }

    @Test fun theThumbnailFitsTheInkNotTheView() { // F17-FR13: so it is cached until the ink changes
        val ink = WorldRect(0.0, 0.0, 5_000.0, 5_000.0)
        val a = MinimapFrame.of(ink, Viewport(0.0, 0.0, 1.0), 2_000.0, 2_400.0, false, 2.0)!!
        val b = MinimapFrame.of(ink, Viewport(9_000.0, 300.0, 0.5), 2_000.0, 2_400.0, false, 2.0)!!
        assertEquals(a.fit, b.fit)
        val f = a.fit
        assertTrue(f.screenX(0.0) >= 16.0 - eps && f.screenX(5_000.0) <= a.width - 16.0 + eps)
        assertTrue(f.screenY(0.0) >= 0 && f.screenY(5_000.0) <= a.height)
        // No ink: the view is what it shows.
        val empty = MinimapFrame.of(null, Viewport(0.0, 0.0, 1.0), 2_000.0, 2_400.0, false, 2.0)!!
        assertEquals(empty.width / 2, empty.fit.screenX(1_000.0), 1e-6)
        assertEquals(empty.height / 2, empty.fit.screenY(1_200.0), 1e-6)
    }

    @Test fun theWindowMarksThePartInViewClampedToTheThumbnail() { // F17-FR13
        val ink = WorldRect(0.0, 0.0, 4_000.0, 4_800.0)
        val f = MinimapFrame.of(ink, Viewport(0.0, 0.0, 1.0), 2_000.0, 2_400.0, false, 2.0)!!
        val w = f.window(Viewport(1_000.0, 1_000.0, 1.0), 2_000.0, 2_400.0)
        assertEquals(f.left + f.fit.screenX(1_000.0), w[0], eps)
        assertEquals(f.top + f.fit.screenY(1_000.0), w[1], eps)
        assertEquals(f.left + f.fit.screenX(3_000.0), w[2], eps)
        val far = f.window(Viewport(-90_000.0, 0.0, 1.0), 2_000.0, 2_400.0)
        assertEquals(f.left, far[0], eps)
        assertEquals(f.left, far[2], eps)
    }

    @Test fun minusAndPlusSplitTheRowAndZoomAboutTheCentre() { // F17-FR13
        val f = MinimapFrame.of(WorldRect(0.0, 0.0, 5_000.0, 5_000.0), Viewport(0.0, 0.0, 1.0), 2_000.0, 2_400.0, false, 2.0)!!
        val y = f.buttonTop + f.buttonH / 2
        assertTrue(f.inMinus(f.left + 1, y) && !f.inPlus(f.left + 1, y))
        assertTrue(f.inMinus(f.split, y) && !f.inPlus(f.split, y))
        assertTrue(f.inPlus(f.split + 1, y) && f.inPlus(f.left + f.width, y))
        assertFalse(f.inMinus(f.left + 1, f.top + 1) || f.inPlus(f.split + 1, f.buttonBottom + 1))
        assertTrue(f.inCard(f.cardLeft + 1, f.cardBottom - 1) && !f.inCard(f.cardRight + 1, y))
        assertFalse(f.contains(f.left + 1, y))
        val view = Viewport(100.0, 200.0, 1.0)
        val zin = f.zoomed(view, zoomIn = true, 2_000.0, 2_400.0)
        assertEquals(1.25, zin.zoom, eps)
        val (cx, cy) = view.centre(2_000.0, 2_400.0)
        val (zx, zy) = zin.centre(2_000.0, 2_400.0)
        assertEquals(cx, zx, 1e-6); assertEquals(cy, zy, 1e-6)
        assertEquals(0.8, f.zoomed(view, zoomIn = false, 2_000.0, 2_400.0).zoom, eps)
    }

    @Test fun aTapOnTheMinimapCentresTheViewThere() { // F17-FR13
        val ink = WorldRect(0.0, 0.0, 5_000.0, 5_000.0)
        val view = Viewport(0.0, 0.0, 1.0)
        val f = MinimapFrame.of(ink, view, 1_200.0, 1_400.0, barOnLeft = false)!!
        // The minimap point showing world (4 000, 3 000).
        val sx = f.left + f.fit.screenX(4_000.0)
        val sy = f.top + f.fit.screenY(3_000.0)
        assertTrue(f.contains(sx, sy))
        val v = f.centreOn(sx, sy, view, 1_200.0, 1_400.0)
        val (cx, cy) = v.centre(1_200.0, 1_400.0)
        assertEquals(4_000.0, cx, 1e-6)
        assertEquals(3_000.0, cy, 1e-6)
        assertEquals(view.zoom, v.zoom, eps)
        // A point off the box is clamped to its edge.
        val (wx, _) = f.worldAt(f.left - 500, sy)
        assertEquals(f.fit.worldX(0.0), wx, 1e-6)
        assertFalse(f.contains(f.left - 1, sy))
    }

    @Test fun draggingTheViewRectanglePansByTheWorldDistance() { // F17-FR13
        val ink = WorldRect(0.0, 0.0, 5_000.0, 5_000.0)
        val view = Viewport(1_000.0, 1_000.0, 1.0)
        val f = MinimapFrame.of(ink, view, 1_200.0, 1_400.0, barOnLeft = false)!!
        val moved = f.dragBy(view, 10.0, -5.0)
        assertEquals(1_000.0 + 10.0 / f.fit.zoom, moved.x, 1e-6)
        assertEquals(1_000.0 - 5.0 / f.fit.zoom, moved.y, 1e-6)
        val inside = f.left + f.fit.screenX(1_500.0)
        val insideY = f.top + f.fit.screenY(1_500.0)
        assertTrue(f.onView(inside, insideY, view, 1_200.0, 1_400.0))
        assertFalse(f.onView(f.left + f.fit.screenX(4_900.0), f.top + f.fit.screenY(4_900.0), view, 1_200.0, 1_400.0))
    }
}
