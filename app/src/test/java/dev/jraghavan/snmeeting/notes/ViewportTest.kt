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

    @Test fun theMinimapFramesInkAndViewAwayFromTheBar() {
        val ink = WorldRect(0.0, 0.0, 5_000.0, 5_000.0)
        val view = Viewport(10_000.0, 10_000.0, 1.0)
        val right = MinimapFrame.of(ink, view, 1_200.0, 1_400.0, barOnLeft = false)!!
        assertEquals(MinimapFrame.MARGIN_PX, right.left, eps)
        val left = MinimapFrame.of(ink, view, 1_200.0, 1_400.0, barOnLeft = true)!!
        assertEquals(1_200.0 - 24.0 - 300.0, left.left, eps)
        assertEquals(1_400.0 - 24.0 - left.height, left.top, eps)
        // Both the ink and the far-away view fit inside the box.
        val f = right.fit
        assertTrue(f.screenX(0.0) >= 0 && f.screenX(11_200.0) <= MinimapFrame.WIDTH_PX)
        assertNull(MinimapFrame.of(ink, view, 0.0, 10.0, false))
        assertTrue(MinimapFrame.of(null, view, 100.0, 100.0, false)!!.height >= 160.0)
        assertTrue(ink.contains(1.0, 1.0))
        assertFalse(ink.contains(-1.0, 1.0))
    }
}
