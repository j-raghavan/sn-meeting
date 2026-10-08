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

    @Test fun anErrorClosesNothing() {
        assertNull(recoverer("""{"error":"nativeInit has not run"}""").run(serviceAlive = true))
        assertEquals(true, alive)
        assertEquals(emptyList<String>(), closed)
    }
}
