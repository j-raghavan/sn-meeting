package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.LiveTranscript
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class StripModelTest { // F17-FR6
    private fun live(json: String) = (CoreJson.obj(json, LiveTranscript::from) as CoreResult.Ok).value

    private val t = live("""{"lines":[{"n":1,"atMs":0,"text":"Hello all."},{"n":2,"atMs":4000,"text":"Budget first."},
        {"n":3,"atMs":761000,"text":"◆ [0:12:41] Mark","mark":"m-1"},{"n":4,"atMs":761500,"text":"We ship Friday."},
        {"n":5,"atMs":800000,"text":"Any questions?"}],"waiting":2,"state":"offline"}""")

    @Test fun theStripShowsTheNewestThreeLinesAndMarks() {
        val s = StripModel(t)
        assertEquals(listOf(3, 4, 5), s.lines.map { it.n })
        assertEquals("m-1", s.lines[0].markId)
        assertEquals("Offline · 2 waiting", s.status)
        assertTrue(s.visible)
        assertEquals(5, s.toggled().lines.size) // open: every line over the lower half
        assertFalse(s.toggled().toggled().open)
        assertTrue(StripModel().lines.isEmpty())
        assertEquals("", StripModel().status)
    }

    @Test fun focusHidesItAndAListenTapScrollsToTheLine() { // F17-AC8, F17-FR7
        assertFalse(StripModel(t, focus = true).visible)
        assertEquals(listOf(1, 2), StripModel(t, highlight = 2).lines.map { it.n })
        assertEquals(listOf(3, 4, 5), StripModel(t, highlight = 99).lines.map { it.n })
    }

    @Test fun itRedrawsOnlyWhenWhatItShowsChanges() { // F17-FR11
        val s = StripModel(t)
        assertTrue(s.same(StripModel(t.copy(spent = "≈ \$0.09"))))
        val more = t.copy(lines = t.lines + t.lines.last().copy(n = 6, text = "No."))
        assertFalse(s.same(StripModel(more)))
        assertFalse(s.same(StripModel(t.copy(state = LiveTranscript.TRANSCRIBING))))
        assertFalse(s.same(s.toggled()))
        assertFalse(s.same(s.copy(focus = true)))
        assertFalse(s.same(s.copy(highlight = 4)))
    }

    @Test fun theLineSpokenAtAMoment() { // F17-FR7, F17-AC4 (±1 line)
        assertEquals(2, StripModel.lineAt(t.lines, 761_000)) // the mark at that moment is skipped
        assertEquals(4, StripModel.lineAt(t.lines, 790_000))
        assertEquals(1, StripModel.lineAt(t.lines, 0))
        assertNull(StripModel.lineAt(t.lines.drop(1), 1_000))
    }
}
