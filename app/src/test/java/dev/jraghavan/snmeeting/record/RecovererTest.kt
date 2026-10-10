package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RecovererTest {
    private val closed = mutableListOf<String>()
    private var alive: Boolean? = null

    private fun recoverer(reply: String) = Recoverer(
        recover = { alive = it; reply },
        duration = { path -> if (path.endsWith("-02.m4a")) 61_234 else null },
        close = { id, index, ms -> closed += "$id/$index=$ms" },
    )

    @Test fun measuresEachOpenSegmentAndClosesItAtZeroWhenUnreadable() {
        val reply = """{"recovered":["a"],"measure":[{"id":"a","index":1,"path":"/p/a-02.m4a"},
            {"id":"b","index":0,"path":"/p/b-01.m4a"}],"flagged":[],"noAudio":["c"]}"""
        val r = recoverer(reply).run(serviceAlive = false)!!
        assertEquals(false, alive)
        assertEquals(listOf("a/1=61234", "b/0=0"), closed)
        assertEquals(listOf("a"), r.recovered)
        assertEquals(listOf("c"), r.noAudio)
    }

    @Test fun legacyAudioIsMeasuredInSegmentOrder() { // D3: no sidecar yet; closing writes one
        val reply = """{"recovered":[],"measure":[{"id":"meeting-20261007-211706","index":0,"path":"/p/meeting-20261007-211706-01.m4a"},
            {"id":"meeting-20261007-211706","index":1,"path":"/p/meeting-20261007-211706-02.m4a"}],"flagged":[]}"""
        recoverer(reply).run(serviceAlive = false)
        assertEquals(listOf("meeting-20261007-211706/0=0", "meeting-20261007-211706/1=61234"), closed)
    }

    @Test fun aMeetingPastRecordedIsMeasuredToo() { // D3: a Transcribe tap gave it a sidecar first
        val reply = """{"recovered":[],"measure":[{"id":"meeting-20261007-211827","index":0,"path":"/p/meeting-20261007-211827-02.m4a"}],"flagged":[]}"""
        val r = recoverer(reply).run(serviceAlive = true)!!
        assertEquals(listOf("meeting-20261007-211827/0=61234"), closed)
        assertEquals(emptyList<String>(), r.recovered)
    }

    @Test fun aSegmentItsMuxerNeverFinishedIsRebuiltFromItsChunks() { // F15-FR8, F15-AC8 (host part)
        val rebuilt = mutableListOf<String>()
        var readable = false
        val r = Recoverer(
            recover = { """{"recovered":["m"],"measure":[
                {"id":"m","index":0,"path":"/p/m-01.m4a","chunks":[{"path":"/f/c-0000.m4a","skipMs":0},{"path":"/f/c-0001.m4a","skipMs":1000}]},
                {"id":"m","index":1,"path":"/p/m-02.m4a","chunks":[{"path":"/f/c-0031.m4a","skipMs":0}]},
                {"id":"n","index":0,"path":"/p/n-01.m4a","chunks":[{"path":"/g/c-0000.m4a","skipMs":0}]}],"flagged":[]}""" },
            duration = { path -> if (path == "/p/m-01.m4a" && readable) 64_000 else null },
            close = { id, index, ms -> closed += "$id/$index=$ms" },
            rebuild = { pieces, target ->
                rebuilt += "$target<" + pieces.joinToString(",") { "${it.path.substringAfterLast('/')}-${it.skipMs}" }
                when (target) {
                    "/p/m-01.m4a" -> { readable = true; true }
                    "/p/m-02.m4a" -> false // couldn't be written: closed at 0 (flagged)
                    else -> throw IllegalStateException("muxer")
                }
            },
        )
        r.run(serviceAlive = false)
        assertEquals(listOf("/p/m-01.m4a<c-0000.m4a-0,c-0001.m4a-1000", "/p/m-02.m4a<c-0031.m4a-0", "/p/n-01.m4a<c-0000.m4a-0"), rebuilt)
        assertEquals(listOf("m/0=64000", "m/1=0", "n/0=0"), closed)
    }

    @Test fun aSegmentWithNoFinishedChunkIsStillRepairedFromItsJournal() { // F15-FR8
        var tried = ""
        Recoverer(
            recover = { """{"recovered":["m"],"measure":[{"id":"m","index":0,"path":"/p/m-01.m4a"}],"flagged":[]}""" },
            duration = { if (tried.isNotEmpty()) 35_000 else null },
            close = { id, index, ms -> closed += "$id/$index=$ms" },
            rebuild = { pieces, target -> tried = "$target:${pieces.size}"; true },
        ).run(serviceAlive = false)
        assertEquals("/p/m-01.m4a:0", tried)
        assertEquals(listOf("m/0=35000"), closed)
    }

    @Test fun anErrorClosesNothing() {
        assertNull(recoverer("""{"error":"nativeInit has not run"}""").run(serviceAlive = true))
        assertEquals(true, alive)
        assertEquals(emptyList<String>(), closed)
    }
}
