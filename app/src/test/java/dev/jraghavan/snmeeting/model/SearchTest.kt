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

    /** F19-FR3: where each kind of hit opens. */
    @Test fun hitsOpenAtTheirPlace() {
        val r = (CoreJson.obj(json, SearchResults::from) as CoreResult.Ok).value
        val (m, e) = r.groups
        fun hit(kind: String, at: Long?) = m.hits[0].copy(kind = kind, atMs = at)
        val id = "meeting-20261009-100000"
        val box = m.hits[0].bbox
        assertEquals(listOf(1.0, 2.0, 3.0, 4.0), box)
        assertNull(m.hits[1].bbox)
        assertEquals(SearchModel.Open.Meeting(id, 759_000, "notes", box), SearchModel.open(m, hit("notes", 761_000)))
        assertEquals(SearchModel.Open.Meeting(id, 0, "notes", box), SearchModel.open(m, hit("notes", 500)))
        assertEquals(SearchModel.Open.Meeting(id, null, "notes", box), SearchModel.open(m, hit("notes", null)))
        assertEquals(SearchModel.Open.Meeting(id, 761_000, "transcript"), SearchModel.open(m, hit("transcript", 761_000)))
        assertEquals(SearchModel.Open.Meeting(id, null, "summary"), SearchModel.open(m, hit("minutes", null)))
        assertEquals(SearchModel.Open.Meeting(id, null, "todos"), SearchModel.open(m, hit("todo", null)))
        assertEquals(SearchModel.Open.Event("l1:u:-", "Budget sync", 1791820800000), SearchModel.open(e, e.hits[0]))
    }

    /** F19-FR3: the paragraph spoken at a moment, and its transcript page. */
    @Test fun aMomentFindsItsParagraph() {
        val segs = listOf(
            Segment("a", 0, 900_000, "one\n\ntwo", null),
            Segment("b", 900_000, 900_000, null, null),
            Segment("c", 1_800_000, 900_000, "x".repeat(2000), null),
        )
        assertEquals(0 to 0, TranscriptPages.locate(segs, 5_000))
        assertEquals(0 to 0, TranscriptPages.locate(segs, 1_000_000))
        assertEquals(1 to 0, TranscriptPages.locate(segs, 1_900_000))
        assertNull(TranscriptPages.locate(listOf(Segment("a", 10, 1, "t", null)), 5))
        assertNull(TranscriptPages.locate(emptyList(), 5))
    }

    @Test fun rebuildReplies() {
        assertEquals("Search index rebuilt  ·  12 meetings and events", SearchModel.rebuilt("""{"indexed":12,"removed":0,"rebuilt":true}"""))
        assertEquals("Search index rebuilt  ·  1 meeting or event", SearchModel.rebuilt("""{"indexed":1}"""))
        assertEquals("The search index is already being updated", SearchModel.rebuilt("""{"busy":true}"""))
        assertEquals("Couldn't rebuild the search index: nativeInit has not run", SearchModel.rebuilt("""{"error":"nativeInit has not run"}"""))
    }
}
