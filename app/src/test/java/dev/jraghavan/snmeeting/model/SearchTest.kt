package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneId

/** F19-FR1 / FR2: reading nativeSearch and nativeIndexStatus, and what Search shows of them. */
class SearchTest {
    private val json = """{"query":"budget","filter":"all","more":true,"totalGroups":60,"indexing":true,"groups":[
        {"kind":"meeting","id":"meeting-20261009-100000","title":null,"start":"2026-10-09T10:00:00-07:00",
         "startMs":1791565200000,"hitCount":23,"hits":[
            {"kind":"notes","label":"NOTES","unitId":"n:0:0","atMs":761000,"time":"0:12:41",
             "snippet":[{"text":"Q3 ","bold":false},{"text":"budget","bold":true},{"text":" ↑","bold":false}],
             "region":0,"bbox":[1,2,3,4],"strokeIds":[4,5],"changed":true},
            {"kind":"minutes","label":"MINUTES","unitId":"summary","atMs":null,"time":null,
             "snippet":[{"text":"The ","bold":false},{"text":"budget","bold":true}],"changed":false}]},
        {"kind":"event","id":"l1:u:-","title":"Budget sync","start":"2026-10-12T09:00:00-07:00",
         "startMs":1791820800000,"hitCount":1,"hits":[{"kind":"event","label":"EVENT","unitId":"event",
         "snippet":[{"text":"Budget","bold":true}]}]},
        {"bad":"group"}]}"""

    @Test fun resultsAreReadWithTheirGroupsAndHits() {
        val r = (CoreJson.obj(json, SearchResults::from) as CoreResult.Ok).value
        assertEquals("budget", r.query)
        assertTrue(r.more && r.indexing)
        assertEquals(60, r.totalGroups)
        assertEquals(2, r.groups.size)
        val m = r.groups[0]
        assertEquals("Untitled meeting", SearchModel.title(m))
        assertEquals("Budget sync", SearchModel.title(r.groups[1]))
        assertEquals("+ 21 more in this meeting", SearchModel.more(m))
        assertNull(SearchModel.more(r.groups[1]))
        val notes = m.hits[0]
        assertEquals(761000L, notes.atMs)
        assertEquals(listOf(Span("Q3 ", false), Span("budget", true), Span(" ↑", false)), notes.snippet)
        assertEquals("NOTES  ·  0:12:41 ▶  ·  (notes changed since recognised)", SearchModel.label(notes))
        assertEquals("MINUTES", SearchModel.label(m.hits[1]))
        assertEquals("Fri 9 Oct 2026  ·  10:00", SearchModel.stamp(m.startMs, ZoneId.of("America/Los_Angeles")))
    }

    @Test fun anErrorStaysAnError() {
        val r = CoreJson.obj("""{"error":"unknown filter"}""", SearchResults::from)
        assertEquals(CoreResult.Err("unknown filter"), r)
    }

    @Test fun filtersAndMessages() {
        assertEquals(listOf("all", "notes", "transcript", "minutes", "todos", "events"), SearchModel.Filter.values().map { it.id })
        assertEquals(listOf("All", "Notes", "Transcript", "Minutes", "To-dos", "Events"), SearchModel.Filter.values().map { it.label })
        assertEquals("No results for “budget” in notes", SearchModel.noResults(" budget ", SearchModel.Filter.NOTES))
        assertEquals("No results for “budget” anywhere", SearchModel.noResults("budget", SearchModel.Filter.ALL))
    }

    @Test fun indexStatusLines() {
        fun s(j: String) = (CoreJson.obj(j, IndexStatus::from) as CoreResult.Ok).value
        assertEquals("Indexing 120/340", SearchModel.indexing(s("""{"state":"indexing","done":120,"total":340}""")))
        assertEquals("Still indexing…", SearchModel.indexing(s("""{"state":"indexing","done":0,"total":0}""")))
        assertEquals("Still indexing…", SearchModel.indexing(s("""{"state":"missing","done":0,"total":0}""")))
        assertNull(SearchModel.indexing(s("""{"state":"ready","done":3,"total":3}""")))
        assertNull(SearchModel.indexing(null))
        assertTrue(CoreJson.obj("""{"done":1}""", IndexStatus::from) is CoreResult.Err)
    }
}
