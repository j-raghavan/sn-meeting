package dev.jraghavan.snmeeting

import org.junit.Assert.assertEquals
import org.junit.Test

class ChunkJobTest { // F15-FR9 / FR10
    private val log = mutableListOf<String>()

    private fun job(toCut: String, cutOk: (ChunkJob.Cut) -> Boolean) = ChunkJob(
        toCut = { id -> log += "toCut $id"; toCut },
        cut = { c -> log += "cut ${c.index} ${c.segPath.substringAfterLast('/')} ${c.fromMs}-${c.toMs}"; cutOk(c) },
        ready = { id, c -> log += "ready $id ${c.index} @${c.startOffsetMs}+${c.durationMs}"; "{}" },
        transcribe = { id -> log += "transcribe $id"; """{"meeting":{}}""" },
    )

    @Test fun goneChunkFilesAreRecutFromTheirSegmentsThenTranscribed() {
        val cuts = """[{"index":1,"segIndex":0,"fromMs":30000,"toMs":60000,"segPath":"/s/m-01.m4a","path":"/f/c-0001.m4a","startOffsetMs":30000,"durationMs":30000},
            {"index":2,"segIndex":0,"fromMs":60000,"toMs":90000,"segPath":"/s/m-01.m4a","path":"/f/c-0002.m4a","startOffsetMs":60000,"durationMs":30000},
            {"index":3,"segIndex":1,"fromMs":0,"toMs":30000,"segPath":"/s/m-02.m4a","path":"/f/c-0003.m4a","startOffsetMs":900000,"durationMs":30000}]"""
        val out = job(cuts) { it.index != 2 && (it.index != 3 || throw IllegalStateException("muxer")) }.run("m")
        assertEquals("""{"meeting":{}}""", out)
        assertEquals(
            listOf("toCut m", "cut 1 m-01.m4a 30000-60000", "ready m 1 @30000+30000", "cut 2 m-01.m4a 60000-90000",
                "cut 3 m-02.m4a 0-30000", "transcribe m"),
            log, // a cut that fails (or throws) is not reported: the core reads it as missing
        )
    }

    @Test fun nothingToCutOrAnErrorStillTranscribes() {
        job("[]") { true }.run("m")
        job("""{"error":"meeting not found"}""") { true }.run("n")
        assertEquals(listOf("toCut m", "transcribe m", "toCut n", "transcribe n"), log)
    }
}
