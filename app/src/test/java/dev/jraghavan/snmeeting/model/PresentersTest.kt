package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PresentersTest {
    private val at = "2026-10-07T21:17:06-07:00"
    private fun row(status: Status, title: String? = null, todos: Int = 0, source: Source = Source.SIDECAR, ms: Long? = 2_530_000) =
        MeetingSummary("m", at, 0, ms, status, null, title, 1, todos, source, "/f")

    @Test fun formats() {
        assertEquals("Wed 7 Oct", Format.day(at))
        assertEquals("Wednesday, 7 October", Format.dayLong(at))
        assertEquals("21:17", Format.time(at)) // the recording's own offset, not the host's zone
        assertEquals("0:42:10", Format.duration(2_530_000))
        assertEquals("—", Format.duration(null))
        assertEquals("", Format.day(""))
        assertEquals("Fri 9 Oct", Format.due("2026-10-09"))
        assertEquals("next week", Format.due("next week"))
    }

    @Test fun meetingRows() {
        val r = HomeRows.meeting(row(Status.DONE, "Q4 planning", todos = 3))
        assertEquals("Q4 planning", r.title)
        assertFalse(r.untitled)
        assertEquals("Wed 7 Oct  ·  21:17", r.stamp)
        assertEquals("0:42:10  ·  3 open to-dos", r.detail)
        assertEquals("Transcribed", r.chip)
        val u = HomeRows.meeting(row(Status.RECORDED, ms = null))
        assertEquals(Format.UNTITLED, u.title)
        assertTrue(u.untitled)
        assertEquals("—", u.detail)
        assertEquals("1 open to-do", HomeRows.meeting(row(Status.TRANSCRIBED, todos = 1)).detail.substringAfter("·  "))
        assertEquals("no open to-dos", HomeRows.meeting(row(Status.DONE)).detail.substringAfter("·  "))
    }

    @Test fun failedAndUnreadableAreInverted() {
        assertEquals("Failed" to true, HomeRows.chip(Status.FAILED, Source.SIDECAR))
        assertEquals("Unreadable" to true, HomeRows.chip(Status.RECORDED, Source.CORRUPT))
        assertEquals("Transcribing…" to false, HomeRows.chip(Status.TRANSCRIBING, Source.SIDECAR))
        for (s in Status.entries) HomeRows.chip(s, Source.SIDECAR) // every status has a label
    }

    @Test fun todoRowsCarryTheMeetingDateDueAndOwner() {
        val t = HomeRows.todo(TodoRef("m", at, null, "t", "Send the deck", "me", "2026-10-09"))
        assertEquals("Wed 7 Oct  ·  Due Fri 9 Oct  ·  Me", t.meta)
        assertEquals("Wed 7 Oct", HomeRows.todo(TodoRef("m", at, null, "t", "x", null, null)).meta)
    }
}
