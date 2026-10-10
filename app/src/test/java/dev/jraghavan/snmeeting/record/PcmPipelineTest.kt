package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class PcmPipelineTest { // F15-FR1 (D14): one capture, one encoder, two writers
    /** 10 240 Hz: a frame is 100 ms; 10 s segments are exactly 100 frames. */
    private val rate = 10_240
    private val dir = File(System.getProperty("java.io.tmpdir"), "chunks/meeting-20261009-100000")
    private val files = mutableMapOf<String, MutableList<Long>>()
    private val log = mutableListOf<String>()
    private var failOpen: String? = null
    private var failClose = false
    private var armNext = true
    private lateinit var p: PcmPipeline

    private val aac = AacFiles { path ->
        if (path == failOpen) throw IllegalStateException("muxer")
        val pts = files.getOrPut(path) { mutableListOf() }
        object : AacFile {
            override fun write(data: ByteArray, ptsUs: Long) { pts += ptsUs }
            override fun close(): Boolean { if (failClose && !path.contains("/c-")) throw IllegalStateException("stop"); return pts.isNotEmpty() }
        }
    }
    private var segment = 1
    private val events = object : PipelineEvents {
        override fun approaching() {
            log += "approaching"
            if (armNext) p.arm("/s/meeting-20261009-100000-%02d.m4a".format(++segment))
        }
        override fun nextFile(offsetMs: Long) { log += "next @$offsetMs" }
        override fun limit() { log += "limit" }
        override fun chunk(c: ChunkDone) { log += "chunk ${c.index} seg ${c.segIndex} @${c.startOffsetMs}+${c.durationMs}" }
        override fun failed(where: String, t: Throwable) { log += "failed $where" }
    }

    private fun pipeline(maxBytes: Long = 1_000_000, base: PcmPipeline.Base = PcmPipeline.Base()) =
        PcmPipeline(rate, 10_000, maxBytes, "/s/meeting-20261009-100000-01.m4a", dir, { _, _ -> null }, aac, events, base, approachMs = 2_000)
            .also { p = it }

    private fun frames(n: Int, bytes: Int = 10) = repeat(n) {
        p.onPcm(ShortArray(1024), 1024, 0)
        p.onFrame(ByteArray(bytes))
    }

    @Test fun everyFrameGoesOnceToItsSegmentAndToAChunk() {
        pipeline()
        frames(250)
        assertTrue(p.finish())
        val segs = files.keys.filter { it.startsWith("/s/") }.sorted()
        assertEquals(listOf(100, 100, 50), segs.map { files.getValue(it).size })
        // Each file's pts start at 0: a segment is a file of its own.
        assertEquals(listOf(0L, 100_000L), files.getValue(segs[1]).take(2))
        assertEquals(
            listOf("approaching", "chunk 0 seg 0 @0+10000", "next @10000", "approaching", "chunk 1 seg 1 @10000+10000",
                "next @20000", "chunk 2 seg 2 @20000+5000"),
            log,
        )
        // The chunks hold every frame exactly once: no gap, no overlap at segment ends.
        assertEquals(250, files.filterKeys { it.contains("/c-") }.values.sumOf { it.size })
        assertEquals(25_000L, p.clock.nowMs) // the recording clock is the samples read
    }

    @Test fun anUnarmedBoundaryReportsTheLimitAndTheFileRunsOn() {
        armNext = false
        pipeline()
        frames(120)
        assertEquals(listOf("approaching", "limit"), log)
        assertEquals(120, files.getValue("/s/meeting-20261009-100000-01.m4a").size)
    }

    @Test fun aRunawayFileIsReportedOnce() { // the size heuristic as a guard only (F15-FR1)
        pipeline(maxBytes = 300)
        frames(40)
        assertEquals(listOf("limit"), log)
    }

    @Test fun aSegmentFileThatCantBeWrittenEndsTheRecording() {
        failOpen = "/s/meeting-20261009-100000-01.m4a"
        pipeline()
        frames(3)
        assertEquals(listOf("failed segment write"), log)
        assertFalse(p.finish())
        failOpen = null
        failClose = true
        log.clear(); files.clear()
        pipeline()
        frames(100) // the roll can't finalise segment 1
        frames(1)
        assertTrue(log.contains("failed segment close"))
        assertFalse(files.containsKey("/s/meeting-20261009-100000-02.m4a"))
    }

    @Test fun aStopWithNothingWrittenHoldsNoAudio() {
        pipeline()
        assertFalse(p.finish())
    }

    @Test fun aRestartNumbersOnFromWhereTheFirstStopped() {
        pipeline()
        frames(150)
        p.finish()
        val reached = p.reached
        assertEquals(PcmPipeline.Base(150, 2, 2), reached)
        log.clear(); segment = 2
        pipeline(base = reached)
        frames(10)
        p.finish()
        assertEquals(listOf("chunk 2 seg 2 @15000+1000"), log)
    }

    @Test fun theChunkFolderIsTheMeetingsOwn() {
        assertEquals(File("/data/files/chunks/meeting-20261009-100000b"),
            PcmPipeline.chunkDir(File("/data/files"), "/storage/emulated/0/EXPORT/sn-meeting/meeting-20261009-100000b-03.m4a"))
    }
}
