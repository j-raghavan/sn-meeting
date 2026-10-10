package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** F20-FR8 / F20-AC5: a tag's recordings, newest first, in pages of the e-ink list. */
class TagListTest {
    private fun row(i: Int, ms: Long) =
        """{"id":"m$i","title":${if (i == 2) "null" else "\"Talk $i\""},"startedAt":"2026-10-0${i}T17:14:00-07:00","startedAtMs":$ms,"durationMs":99000,"tags":["local-ai","microsoft"]}"""

    private val json = """{"tag":"gpu","count":5,"meetings":[${row(5, 500)},${row(3, 300)},${row(4, 400)},${row(2, 200)},${row(1, 100)},{"title":"no id"}]}"""

    @Test
    fun the_list_is_newest_first_and_paged() {
        val l = TagList.from(JSONObject(json))!!
        assertEquals(listOf("m5", "m4", "m3", "m2", "m1"), l.meetings.map { it.id })
        assertEquals("#gpu  ·  5 recordings  ·  newest first", l.header)
        assertEquals(2, l.pages())
        assertEquals(listOf("m5", "m4", "m3", "m2"), l.page(0).map { it.id })
        assertEquals(listOf("m1"), l.page(1).map { it.id })
        assertEquals(listOf("m1"), l.page(9).map { it.id })
        assertEquals("2 / 2", l.pageLabel(7))
        assertEquals("1 / 1", TagList("gpu", emptyList()).pageLabel(0))
        assertNull(TagList.from(JSONObject("{}")))
    }

    @Test
    fun a_row_reads_as_title_stamp_and_other_tags() {
        val l = TagList.from(JSONObject(json))!!
        val m = l.meetings.first()
        assertEquals("Talk 5", m.shownTitle)
        assertEquals("Mon 5 Oct  ·  17:14  ·  1:39", m.stamp)
        assertEquals("#local-ai  #microsoft", m.tagLine)
        assertEquals(Format.UNTITLED, l.meetings.first { it.id == "m2" }.shownTitle)
        assertEquals("#gpu  ·  1 recording  ·  newest first", TagList("gpu", l.page(1)).header)
    }
}
