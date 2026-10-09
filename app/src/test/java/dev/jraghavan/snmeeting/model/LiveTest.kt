package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveTest { // F15-FR6 / FR7, F17-FR6
    private fun live(json: String) = (CoreJson.obj(json, LiveTranscript::from) as CoreResult.Ok).value

    @Test fun theStripShowsTheNewestThreeWholeLinesAndWhereTheLaneIs() {
        val t = live("""{"lines":[{"n":1,"atMs":0,"text":"One."},{"n":2,"atMs":3000,"text":"Two."},
            {"n":3,"atMs":6000,"text":"Three."},{"n":4,"atMs":9000,"text":"Four."}],
            "waiting":4,"state":"offline","reason":"No connection","spentUsd":0.006,"spent":"< $0.01"}""")
        assertEquals(listOf("Two.", "Three.", "Four."), t.shown(all = false).map { it.text })
        assertEquals(4, t.shown(all = true).size)
        assertEquals("Offline · 4 waiting", t.status)
        assertEquals("< $0.01", t.spent)
        assertEquals("Transcribing…", t.copy(state = LiveTranscript.TRANSCRIBING).status)
        assertEquals("Transcription paused: OpenAI rejected the key (401)",
            t.copy(state = LiveTranscript.PAUSED, reason = "Transcription paused: OpenAI rejected the key (401)").status)
        assertEquals("Transcription paused", t.copy(state = LiveTranscript.PAUSED, reason = null).status)
        assertEquals("", t.copy(state = "current").status)
        val empty = live("""{"lines":[],"waiting":0,"state":"off"}""")
        assertTrue(empty.shown(false).isEmpty())
        assertEquals("—", empty.spent)
    }

    @Test fun aNearLiveMeetingPagesItsChunkParagraphs() {
        val json = """{"meeting":{"id":"meeting-20261009-100000","status":"recording","live":{"provider":"openai"},
            "segments":[{"file":"meeting-20261009-100000-01.m4a","startOffsetMs":0,"text":null}]},
            "paragraphs":[{"index":0,"atMs":0,"text":"And then we ship it."},{"index":1,"atMs":29000,"text":"Next, the budget."}]}"""
        val v = (CoreJson.obj(json, MeetingView::from) as CoreResult.Ok).value
        assertTrue(v.meeting.nearLive)
        assertEquals(listOf(0L, 29_000L), v.transcript.map { it.startOffsetMs })
        assertEquals(listOf("0:00:00", "0:00:29"), TranscriptPages.paragraphs(v.transcript).map { it.marker })
        // A Standard meeting pages its segments, as before.
        val plain = (CoreJson.obj("""{"meeting":{"id":"meeting-20261009-100000","status":"done",
            "segments":[{"file":"a-01.m4a","startOffsetMs":0,"text":"Hello."}]}}""", MeetingView::from) as CoreResult.Ok).value
        assertFalse(plain.meeting.nearLive)
        assertEquals("Hello.", plain.transcript.single().text)
    }
}
