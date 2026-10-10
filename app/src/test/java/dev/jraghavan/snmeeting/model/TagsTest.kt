package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** F20: how tags read in the shell, from the core's replies. */
class TagsTest {
    @Test
    fun tags_read_and_show_with_a_hash() {
        assertEquals("#local-ai", Tags.shown("local-ai"))
        assertTrue(Tags.removeQuestion("gpu").startsWith("Remove #gpu from this meeting?"))
        val o = JSONObject("""{"tags":[{"name":"gpu","source":"ai"},{"name":"vram","source":"user"},{"source":"ai"}]}""")
        assertEquals(listOf("gpu", "vram"), Tags.names(o))
        assertEquals(emptyList<String>(), Tags.names(JSONObject("{}")))
    }

    /** F20-FR6: a Home row's line, at most three, in the core's order. */
    @Test
    fun home_rows_show_up_to_three_tags() {
        assertEquals("#gpu  #vram  #local-ai", Tags.line(listOf("gpu", "vram", "local-ai", "q4")))
        assertEquals("", Tags.line(emptyList()))
        val r = AgendaRow.from(JSONObject("""{"kind":"meeting","meetingId":"m","startMs":1,"tags":["gpu","vram"]}"""))!!
        assertEquals(listOf("gpu", "vram"), r.tags)
        assertEquals(emptyList<String>(), AgendaRow.from(JSONObject("""{"kind":"meeting","startMs":1}"""))!!.tags)
    }

    @Test
    fun a_sidecar_lists_its_tags() {
        val sc = Sidecar.from(JSONObject("""{"id":"meeting-20261009-171436","status":"done",
            "tags":[{"name":"local-ai","source":"ai"},{"name":"gpu","source":"user"}]}"""))!!
        assertEquals(listOf("local-ai", "gpu"), sc.tags)
        assertEquals(emptyList<String>(), Sidecar.from(JSONObject("""{"id":"m","status":"done"}"""))!!.tags)
    }
}
