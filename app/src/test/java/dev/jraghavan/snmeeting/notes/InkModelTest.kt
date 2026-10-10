package dev.jraghavan.snmeeting.notes

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class InkModelTest { // F17-FR1 / FR4: what the canvas draws, from nativeInkStrokesForDraw
    private val reply = """{"strokes":[{"id":0,"tool":0,"width":3,"color":[0,0,0,255],"atMs":450000,"pts":[0,0,40,0]},
        {"id":1,"tool":1,"width":30,"color":[0,0,0,64],"atMs":null,"pts":[10,10]},
        {"id":2,"pts":[]},{"tool":0,"pts":[1,1]}],"bounds":[-1.5,-15,41.5,25],"canUndo":true,"canRedo":false}"""

    @Test fun readsStrokesBoundsAndHistory() {
        val v = InkView.from(JSONObject(reply))
        assertEquals(listOf(0, 1), v.strokes.map { it.id })
        val pen = v.strokes[0]
        assertEquals(450_000L, pen.atMs)
        assertEquals(0xFF000000.toInt(), pen.argb)
        assertFalse(pen.highlighter)
        assertEquals(WorldRect(-1.5, -1.5, 41.5, 1.5), pen.bounds)
        val hl = v.strokes[1]
        assertTrue(hl.highlighter)
        assertNull(hl.atMs)
        assertEquals(0x40000000, hl.argb)
        assertEquals(WorldRect(-1.5, -15.0, 41.5, 25.0), v.bounds)
        assertTrue(v.canUndo && !v.canRedo)
        assertEquals(InkView.EMPTY, InkView.from(JSONObject("{}")))
        assertEquals(0xFF000000.toInt(), DrawStroke.from(JSONObject("""{"id":5,"pts":[1,2]}"""))!!.argb)
    }

    @Test fun playbackMutesWhatWasWrittenLater() { // F17-FR7 / F13-FR3
        val v = InkView.from(JSONObject(reply))
        val (pen, hl) = v.strokes
        assertFalse(Emphasis.muted(pen, null))
        assertTrue(Emphasis.muted(pen, 449_999))
        assertFalse(Emphasis.muted(pen, 450_000))
        assertFalse("untimed stays as it is", Emphasis.muted(hl, 0))
        assertTrue(Emphasis.changed(v.strokes, 449_000, 451_000))
        assertFalse(Emphasis.changed(v.strokes, 460_000, 470_000))
        assertTrue(Emphasis.changed(v.strokes, 449_000, null))
    }

    @Test fun aMarkDrawsADiamondBesideTheStrokeBeforeIt() { // F17-FR3
        val v = InkView.from(JSONObject(reply))
        assertEquals(listOf(49.5 to -1.5), MarkGlyphs.place(v.strokes, listOf(450_000L, 460_000L)))
        assertTrue(MarkGlyphs.place(v.strokes, listOf(10L)).isEmpty())
    }

    @Test fun aListenHitSaysWhereToPlayFrom() { // F17-FR7
        val h = ListenHit.from(JSONObject("""{"strokeId":4,"atMs":450000,"seekMs":448000,"segIndex":0,"segOffsetMs":448000,"lineN":7}"""))
        assertEquals(ListenHit(4, 450_000, 448_000, 7), h)
        assertEquals(ListenHit(1, null, null, null), ListenHit.from(JSONObject("""{"strokeId":1,"atMs":null}""")))
        assertEquals(ListenHit(null, null, null, null), ListenHit.from(JSONObject("""{"strokeId":null}""")))
    }
}
