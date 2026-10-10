package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.DrawEntry
import dev.jraghavan.snmeeting.notes.InkView
import dev.jraghavan.snmeeting.notes.WorldRect
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI

class DrawItemTest { // F21-FR1: what nativeCanvasForDraw hands the painters
    private val reply = """{"entries":[
        {"ref":"s:0","stroke":{"id":0,"tool":0,"width":3,"color":[0,0,0,255],"atMs":5,"pts":[0,0,40,0],
          "style":{"color":"red","size":"l"},"groupId":"g"}},
        {"ref":"i:a","item":{"id":"a","type":"rectangle","known":true,"x":0,"y":0,"width":100,"height":50,
          "rotation":0,"style":{"color":"blue","fill":"semi"},"recMs":761000,"groupId":"g",
          "link":{"kind":"meeting","target":"meeting-20261008-090000","followed":true}}},
        {"ref":"i:ar","item":{"id":"ar","type":"arrow","known":true,"x":0,"y":0,"width":0,"height":0,
          "rotation":0,"ends":[100,25,400,25],"bound":[true,false],"style":{}}},
        {"ref":"i:t","item":{"id":"t","type":"table","known":true,"x":0,"y":0,"width":320,"height":96,
          "rotation":0,"style":{},"table":{"rows":2,"cols":2,"cells":["a","b","c","d"],"rowHeights":[48,48]},
          "image":{"file":"t.png","width":4,"height":3}}},
        {"ref":"i:bad","item":{"type":"x"}},
        {"ref":"i:u","item":{"id":"u","type":"sparkle","x":1,"y":2,"width":3,"height":4,
          "table":{"rows":2,"cols":2,"cells":["a"],"rowHeights":[1]}}},
        {"stroke":{"id":9,"pts":[0,0]}}],
        "bounds":[0,0,400,96],"canUndo":true,"canRedo":false,"selection":["i:a","s:0"],
        "selectionBounds":[0,0,100,50],"itemsOk":false}"""

    @Test fun readsEntriesInPaintOrder() {
        val v = InkView.from(JSONObject(reply))
        assertEquals(listOf("s:0", "i:a", "i:ar", "i:t", "i:u"), v.entries.map { it.ref })
        assertEquals(listOf(0), v.strokes.map { it.id })
        assertEquals(StyleColor.RED, v.strokes[0].style!!.color)
        assertEquals("g", v.strokes[0].groupId)
        val a = (v.entries[1] as DrawEntry.Thing).item
        assertEquals(761_000L, a.recMs)
        assertEquals(FillStyle.SEMI, a.style.fill)
        assertTrue(a.link!!.followed)
        assertEquals(WorldRect(-1.75, -1.75, 101.75, 51.75), a.bounds)
        val ar = v.items[1]
        assertTrue(ar.hasEnds)
        assertEquals(true to false, ar.bound)
        assertEquals(WorldRect(98.25, 23.25, 401.75, 26.75), ar.bounds)
        val t = v.items[2]
        assertEquals(listOf(48.0, 48.0), t.table!!.rowHeights)
        assertEquals(DrawImage("t.png", 4, 3), t.image)
        val u = v.items[3]
        assertFalse(u.known)
        assertNull("a table that doesn't add up is left off", u.table)
        assertEquals(listOf("i:a", "s:0"), v.selection)
        assertEquals(WorldRect(0.0, 0.0, 100.0, 50.0), v.selectionBounds)
        assertFalse(v.itemsOk)
    }

    @Test fun rotatedCornersAndFrames() {
        val r = DrawItem("i:r", "r", "rectangle", true, 0.0, 0.0, 100.0, 50.0, PI / 2, null)
        val c = r.corners()
        assertEquals(75.0, c[0].x, 1e-9)
        assertEquals(-25.0, c[0].y, 1e-9)
        val back = r.toLocal(r.toWorld(12.0, 34.0).x, r.toWorld(12.0, 34.0).y)
        assertEquals(12.0, back.x, 1e-9)
        assertEquals(34.0, back.y, 1e-9)
    }
}
