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

    private fun sc(status: Status = Status.RECORDED, failedAt: FailedAt? = null, texts: List<String?> = listOf(null),
                   title: String? = null, note: String? = null, mic: MicRef? = MicRef("usb", "USB-Audio - USB Audio")) = Sidecar(
        "m", at, 2_530_000, status, failedAt, title, null, null, null, null, null, emptyList(), emptyList(), mic,
        texts.mapIndexed { i, t -> Segment("m-0${i + 1}.m4a", i * 900_000L, 900_000, t, null) }, null, note, emptyList())

    @Test fun meetingHeader() {
        val h = MeetingHeader.of(sc(texts = listOf(null, null, null), note = "mic disconnected"))
        assertEquals(Format.UNTITLED, h.title)
        assertTrue(h.untitled)
        assertEquals("Wednesday, 7 October  ·  21:17", h.stamp)
        assertEquals("0:42:10  ·  3 segments", h.length)
        assertEquals("USB — USB-Audio - USB Audio", h.mic)
        assertEquals("Recording ended: mic disconnected", h.note)
        val b = MeetingHeader.of(sc(title = "Hiring sync", mic = MicRef("bluetooth", "Jabra"), note = ""))
        assertEquals("Hiring sync", b.title)
        assertEquals("0:42:10  ·  1 segment", b.length)
        assertEquals("Bluetooth — Jabra", b.mic)
        assertEquals(null, b.note)
        assertEquals(null, MeetingHeader.of(sc(mic = null)).mic) // legacy audio
    }

    private val found = KeyStatus.Found("copilot-key-openai.txt", "gpt-4o-mini", null)
    private val missing = KeyStatus.Problem("missing", "copilot-key-gemini.txt", "Add copilot-key-gemini.txt to MyStyle/SnCopilot")

    @Test fun everyStatusHasAnEnabledActionOrProgress() { // INV8
        for (st in Status.entries) for (f in listOf(null) + FailedAt.entries) for (running in listOf(false, true)) {
            val a = Actions.of(sc(st, f), jobRunning = running, transcribeKey = found, summariseKey = found)
            assertTrue("$st/$f/$running", (a.primary != null && a.primaryEnabled) || a.progress != null)
        }
    }

    @Test fun theActionFollowsTheStatus() { // F7-FR2
        fun job(st: Status, f: FailedAt? = null) = Actions.of(sc(st, f), false, found, found).primary
        assertEquals(Actions.Job.TRANSCRIBE, job(Status.RECORDED))
        assertEquals(Actions.Job.TRANSCRIBE, job(Status.FAILED, FailedAt.TRANSCRIBING))
        assertEquals(Actions.Job.TRANSCRIBE, job(Status.FAILED)) // no failedAt: start over
        assertEquals(Actions.Job.SUMMARISE, job(Status.FAILED, FailedAt.SUMMARISING))
        assertEquals(Actions.Job.SUMMARISE, job(Status.TRANSCRIBED))
        assertEquals(Actions.Job.SUMMARISE, job(Status.DONE))
        assertEquals(null, job(Status.RECORDING))
    }

    @Test fun progressAndCancel() {
        val a = Actions.of(sc(Status.TRANSCRIBING, texts = listOf("a", null, null)), true, found, found)
        assertEquals("Transcribing 2/3", a.progress)
        assertTrue(a.canCancel)
        assertFalse(a.canDelete)
        val s = Actions.of(sc(Status.DONE), true, found, found)
        assertEquals("Summarising…", s.progress)
        assertTrue(s.canCancel)
        assertEquals("Transcribing 3/3", Actions.transcribing(sc(Status.TRANSCRIBING, texts = listOf("a", "b", "c"))))
    }

    @Test fun aMissingKeyDisablesItsActionWithTheReason() { // F7-AC2
        val b = Actions.of(sc(Status.RECORDED), false, missing, found)
        assertFalse(b.primaryEnabled)
        assertEquals(missing.detail, b.note)
        val c = Actions.of(sc(Status.DONE), false, found, missing)
        assertFalse(c.primaryEnabled)
        assertTrue(Actions.of(sc(Status.RECORDED), false, found, missing).primaryEnabled) // transcribe needs only its own key
        assertEquals("Not transcribed yet.", Actions.of(sc(Status.RECORDED), false, found, found).note)
    }
}
