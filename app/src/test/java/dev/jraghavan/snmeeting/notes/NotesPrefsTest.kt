package dev.jraghavan.snmeeting.notes

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotesPrefsTest { // F17-FR2: the docked bar's side and collapsed state, persisted
    @Test fun defaultsAreRightExpandedNotesUsed() {
        val p = NotesPrefs.from(null)
        assertEquals(NotesPrefs.Dock.RIGHT, p.dock)
        assertFalse(p.collapsed)
        assertFalse(p.focus)
        assertTrue(p.useForMinutes)
        assertEquals(false, p.noticeShown["anthropic"])
    }

    @Test fun readsTheStoredChoicesAndNeverTop() {
        val p = NotesPrefs.from(JSONObject("""{"dock":"left","sidebar":"collapsed","focus":true,"useForMinutes":false,"noticeShown":{"openai":true}}"""))
        assertEquals(NotesPrefs(NotesPrefs.Dock.LEFT, true, true, false, mapOf("openai" to true, "gemini" to false, "anthropic" to false)), p)
        assertEquals(NotesPrefs.Dock.RIGHT, NotesPrefs.from(JSONObject("""{"dock":"top"}""")).dock) // OQ13: no top
        assertEquals(listOf("right", "left"), NotesPrefs.Dock.entries.map { it.wire })
    }

    @Test fun patchesHaveTheCoresShape() {
        assertEquals("""{"notes":{"dock":"left"}}""", NotesPatch.dock(NotesPrefs.Dock.LEFT))
        assertEquals("""{"notes":{"sidebar":"collapsed"}}""", NotesPatch.collapsed(true))
        assertEquals("""{"notes":{"sidebar":"expanded"}}""", NotesPatch.collapsed(false))
        assertEquals("""{"notes":{"focus":true}}""", NotesPatch.focus(true))
        assertEquals("""{"notes":{"useForMinutes":false}}""", NotesPatch.useForMinutes(false))
        assertEquals("""{"notes":{"noticeShown":{"gemini":true}}}""", NotesPatch.noticeShown("gemini"))
    }

    @Test fun theCanvasStartsBesideTheBarAndWidensWhenItCollapses() { // F17-AC1 / AC1b geometry
        val right = DockLayout.spans(1404, NotesPrefs.Dock.RIGHT, collapsed = false, expandedPx = 132, collapsedPx = 64)
        assertEquals(DockLayout.Spans(1272, 1404, 0, 1272), right)
        val left = DockLayout.spans(1404, NotesPrefs.Dock.LEFT, collapsed = false, expandedPx = 132, collapsedPx = 64)
        assertEquals(DockLayout.Spans(0, 132, 132, 1404), left)
        val thin = DockLayout.spans(1404, NotesPrefs.Dock.RIGHT, collapsed = true, expandedPx = 132, collapsedPx = 64)
        assertTrue(thin.canvasWidth > right.canvasWidth)
        assertEquals(1340, thin.canvasWidth)
        assertEquals(0, DockLayout.spans(50, NotesPrefs.Dock.LEFT, false, 132, 64).canvasWidth)
    }

    @Test fun theNotesNoticeShowsOncePerProviderWhileTheSettingIsOn() { // OQ20, F17-FR9
        val p = NotesPrefs()
        assertTrue(NotesNotice.of(p, "anthropic", "Anthropic")!!.contains("go to Anthropic"))
        assertEquals(null, NotesNotice.of(p.copy(noticeShown = mapOf("anthropic" to true)), "anthropic", "Anthropic"))
        assertEquals(null, NotesNotice.of(p.copy(useForMinutes = false), "anthropic", "Anthropic"))
        assertEquals(null, NotesNotice.of(p, null, ""))
    }
}
