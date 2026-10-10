package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** F20-FR7 / FR9: the still drawing of the core's layout: Fit, sizes, zoom, taps, edges. */
class TagGraphTest {
    private val json = """{"range":"all","laidOut":true,"graph":{"meetings":5,"bounds":[-3,-1,3,1],
        "nodes":[{"name":"gpu","count":5,"x":-2,"y":0,"r":1},{"name":"vram","count":2,"x":2,"y":0,"r":0.63},{"count":1}],
        "edges":[{"a":0,"b":1,"shared":2,"weak":false},{"a":0,"b":1,"shared":1,"weak":true},{"a":0}]}}"""
    private val g = TagGraph.from(JSONObject(json))!!

    @Test
    fun the_graph_reads() {
        assertEquals(listOf("gpu", "vram"), g.nodes.map { it.name })
        assertEquals(2, g.edges.size)
        assertEquals(5, g.meetings)
        assertEquals(listOf(-3.0, -1.0, 3.0, 1.0), g.bounds)
        assertEquals(1, g.index("vram"))
        assertNull(g.index("q4"))
        assertNull(TagGraph.from(JSONObject("{}")))
        assertEquals(TagGraph.drawKey(json), TagGraph.drawKey(json.replace("\"laidOut\":true", "\"laidOut\":false")))
        assertEquals(listOf(0.0, 0.0, 0.0, 0.0), TagGraph.from(JSONObject("""{"graph":{}}"""))!!.bounds)
    }

    @Test
    fun fit_centres_the_graph_and_keeps_the_largest_circle_to_a_quarter_of_the_width() {
        val v = TagGraphMath.fit(g, 800, 600, 0)
        assertEquals(400.0, v.x(0.0), 1e-9)
        assertEquals(300.0, v.y(0.0), 1e-9)
        assertTrue("diameter ≤ w/4", 2 * v.scale <= 800 / 4.0 + 1e-9)
        assertEquals(100.0, v.scale, 1e-9) // 800/6 would be larger: the w/8 cap wins
        val wide = TagGraphMath.fit(g, 4000, 400, 20)
        assertEquals(180.0, wide.scale, 1e-9) // the height limits it: (400 - 40) / 2
        assertEquals(0.0, v.worldX(v.x(0.0)), 1e-9)
        assertEquals(1.5, v.worldY(v.y(1.5)), 1e-9)
    }

    @Test
    fun circles_stay_tappable_and_bounded() {
        val v = Viewport(10.0, 0.0, 0.0)
        assertEquals(48.0, TagGraphMath.radiusPx(1.0, v, 48.0, 200.0), 1e-9)
        assertEquals(200.0, TagGraphMath.radiusPx(30.0, v, 48.0, 200.0), 1e-9)
        assertEquals(48.0, TagGraphMath.radiusPx(30.0, v, 48.0, 10.0), 1e-9)
    }

    @Test
    fun zoom_holds_the_focus_and_stays_in_range() {
        val fit = Viewport(100.0, 400.0, 300.0)
        val z = TagGraphMath.zoom(fit, 2.0, 500.0, 300.0, fit)
        assertEquals(200.0, z.scale, 1e-9)
        assertEquals(fit.worldX(500.0), z.worldX(500.0), 1e-9)
        assertEquals(800.0, TagGraphMath.zoom(fit, 100.0, 0.0, 0.0, fit).scale, 1e-9)
        assertEquals(25.0, TagGraphMath.zoom(fit, 0.001, 0.0, 0.0, fit).scale, 1e-9)
        val p = TagGraphMath.pan(fit, 10.0, -5.0)
        assertEquals(410.0, p.ox, 1e-9); assertEquals(295.0, p.oy, 1e-9)
    }

    @Test
    fun a_tap_finds_the_circle_under_it() {
        val v = TagGraphMath.fit(g, 800, 600, 0)
        assertEquals(0, TagGraphMath.hit(g, v, v.x(-2.0), v.y(0.0), 48.0, 100.0))
        assertEquals(1, TagGraphMath.hit(g, v, v.x(2.0) + 40, v.y(0.0), 48.0, 100.0))
        assertNull(TagGraphMath.hit(g, v, 400.0, 30.0, 48.0, 100.0))
    }

    @Test
    fun weak_edges_show_only_for_the_selected_tag() {
        assertEquals(1, TagGraphMath.visibleEdges(g, null).size)
        assertEquals(2, TagGraphMath.visibleEdges(g, 1).size)
        assertTrue(TagGraphMath.strokePx(4, 2.0) > TagGraphMath.strokePx(1, 2.0))
        assertEquals(2.0, TagGraphMath.strokePx(0, 2.0), 1e-9)
    }

    @Test
    fun labels() {
        assertEquals("#gpu  5", TagGraphMath.label(g.nodes[0]))
        assertTrue(TagGraphMath.fitsInside(80f, 50.0))
        assertTrue(!TagGraphMath.fitsInside(90f, 50.0))
        assertEquals(listOf("all", "3m"), TagGraphMath.Range.values().map { it.id })
    }
}
