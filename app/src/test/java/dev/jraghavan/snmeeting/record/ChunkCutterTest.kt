package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class ChunkCutterTest { // F15-FR4
    /** 10 240 Hz: one AAC frame is exactly 100 ms, one RMS block is 1 024 samples. */
    private val rate = 10_240
    private val dir = File(System.getProperty("java.io.tmpdir"), "chunks/meeting-20261009-100000")
    private val written = mutableMapOf<String, MutableList<Pair<Int, Long>>>()
    private val done = mutableListOf<ChunkDone>()
    private val asked = mutableListOf<Pair<Int, Long?>>()
    private var plans = ArrayDeque<ChunkPlan?>()
    private var failWrite = false

    private val planner = ChunkPlanner { rms, limit -> asked += rms.size to limit; if (plans.isEmpty()) null else plans.removeFirst() }
    private val files = AacFiles { path ->
        if (failWrite) throw IllegalStateException("no space")
        val frames = written.getOrPut(path) { mutableListOf() }
        object : AacFile {
            override fun write(data: ByteArray, ptsUs: Long) { frames += data[0].toInt() to ptsUs }
            override fun close() = true
        }
    }
    private val rms = RmsMeter(rate)
    private val cutter = ChunkCutter(rate, dir, planner, files, rms, { done += it })
    private var frame = 0L

    /** [secs] of frames (10 a second) in segment [seg]; each frame's byte is its index mod 128. */
    private fun feed(secs: Int, seg: Int = 0) = repeat(secs * 10) {
        rms.onPcm(ShortArray(1024) { 1000 }, 1024)
        cutter.add(AacFrame(frame, byteArrayOf((frame % 128).toByte())), seg)
        frame++
    }

    @Test fun framesAndMs() {
        assertEquals(100L, framesToMs(1, rate))
        assertEquals(350L, msToFrames(35_000, rate))
        assertEquals(43L, msToFrames(1_000, 44_100)) // 1 s at 44.1 kHz is 43 frames (998 ms)
        assertEquals(998L, framesToMs(43, 44_100))
        assertEquals("c-0007.m4a", ChunkCutter.chunkName(7))
    }

    @Test fun aPauseCutWritesTheFramesUpToItAndKeepsTheRestForTheNextChunk() {
        plans += ChunkPlan(28_400, 0, 26_000)
        feed(35)
        assertEquals(listOf(350 to null), asked) // decided at 35 s of audio, blocks from the chunk start
        val c = done.single()
        assertEquals(ChunkDone(0, "$dir/c-0000.m4a", 0, 28_400, 0, 26_000, 0, true), c)
        val frames = written.getValue(c.path)
        assertEquals(284, frames.size)
        assertEquals(0 to 0L, frames.first())
        assertEquals(283L * 100_000, frames.last().second) // pts from the chunk's own start
        // The next chunk starts at the cut: 6.6 s already held, decided at 35 s of its own.
        plans += ChunkPlan(30_000, 0, 30_000)
        feed(28)
        assertEquals(1, done.size)
        feed(1)
        assertEquals(ChunkDone(1, "$dir/c-0001.m4a", 28_400, 30_000, 0, 30_000, 0, true), done[1])
        assertEquals(284 % 128, written.getValue(done[1].path).first().first)
    }

    @Test fun aHardCutRepeatsTheLastSecondInTheNextChunk() {
        plans += ChunkPlan(35_000, 1_000, 35_000)
        feed(35)
        assertEquals(35_000L, done[0].durationMs)
        plans += ChunkPlan(35_000, 1_000, 35_000)
        feed(33)
        assertEquals(1, done.size)
        feed(1) // 1 s repeated + 34 s new = 35 s held
        val second = done[1]
        assertEquals(34_000L, second.startOffsetMs) // F15-AC1: the next chunk starts at 34 s
        assertEquals(1_000L, second.overlapMs)
        assertEquals(35_000L, second.durationMs)
        assertEquals(340 % 128, written.getValue(second.path).first().first)
    }

    @Test fun aSegmentEndCutsThereWithNoOverlap() {
        feed(12)
        plans += ChunkPlan(12_000, 0, 9_000)
        cutter.endSegment()
        assertEquals(listOf(120 to 12_000L), asked)
        assertEquals(ChunkDone(0, "$dir/c-0000.m4a", 0, 12_000, 0, 9_000, 0, true), done[0])
        // The next segment's chunk starts with the next frame and carries its segment index.
        feed(3, seg = 1)
        plans += ChunkPlan(3_000, 0, 3_000)
        cutter.finish()
        assertEquals(ChunkDone(1, "$dir/c-0001.m4a", 12_000, 3_000, 0, 3_000, 1, true), done[1])
    }

    @Test fun aLimitCutUsesEveryHeldFrameWhateverTheCoreRoundsTo() {
        feed(5)
        plans += ChunkPlan(4_900, 1_000, 5_000) // limits never overlap and never drop frames
        cutter.finish()
        assertEquals(5_000L, done[0].durationMs)
        assertEquals(0L, done[0].overlapMs)
    }

    @Test fun stopRightAfterAHardCutWritesNothingNew() {
        plans += ChunkPlan(35_000, 1_000, 35_000)
        feed(35)
        cutter.finish() // only the repeated second is held
        assertEquals(1, done.size)
        cutter.finish()
        assertEquals(1, done.size)
        assertEquals(1, cutter.count)
    }

    @Test fun aPlannerThatCantAnswerGetsAHardCut() {
        feed(35) // no plan: 35 s, 1 s overlap
        assertEquals(ChunkDone(0, "$dir/c-0000.m4a", 0, 35_000, 0, -1, 0, true), done[0])
        feed(4)
        cutter.finish() // no plan at Stop: the whole of what is held
        assertEquals(34_000L, done[1].startOffsetMs)
        assertEquals(5_000L, done[1].durationMs)
        assertEquals(-1L, done[1].speechMs)
    }

    @Test fun aChunkThatCantBeWrittenIsStillReportedAndNeverStopsTheRecording() { // INV10
        failWrite = true
        val logged = mutableListOf<String>()
        val c = ChunkCutter(rate, dir, planner, files, rms, { done += it }) { what, _ -> logged += what }
        repeat(50) { c.add(AacFrame(it.toLong(), byteArrayOf(0)), 0) }
        plans += ChunkPlan(5_000, 0, 5_000)
        c.finish()
        assertEquals(false, done.single().written)
        assertEquals(listOf("chunk 0"), logged)
    }

    @Test fun rmsBlocksFollowTheChunk() {
        val m = RmsMeter(rate)
        m.onPcm(ShortArray(1024) { 3 }, 1024)
        m.onPcm(ShortArray(1024) { 4 }, 512) // half a block: not yet complete
        assertArrayEquals(doubleArrayOf(3.0), m.from(0), 1e-9)
        m.onPcm(ShortArray(1024) { 4 }, 512)
        assertArrayEquals(doubleArrayOf(3.0, 4.0), m.from(0), 1e-9)
        assertArrayEquals(doubleArrayOf(4.0), m.from(1024), 1e-9)
        m.drop(1024)
        assertArrayEquals(doubleArrayOf(4.0), m.from(0), 1e-9) // dropped blocks are gone
        assertArrayEquals(doubleArrayOf(4.0), m.from(1500), 1e-9)
        assertEquals(0, m.from(99_999).size)
        assertEquals(1600, RmsMeter(16_000).blockSamples) // 100 ms at 16 kHz
    }

    @Test fun theCorePlannerSpeaksNativeChunkPlansJson() {
        var sent: Pair<String, String>? = null
        val p = CorePlanner("meeting-x") { id, json -> sent = id to json; """{"cutAtMs":28450,"overlapMs":0,"speechMs":26100}""" }
        assertEquals(ChunkPlan(28_450, 0, 26_100), p.plan(doubleArrayOf(1.25, 700.0), 12_000))
        assertEquals("meeting-x" to """{"rms":[1.3,700],"limitMs":12000}""", sent)
        assertEquals(ChunkPlan(null, 0, 0), CorePlanner("m") { _, _ -> """{"cutAtMs":null,"overlapMs":0,"speechMs":0}""" }.plan(doubleArrayOf(), null))
        assertNull(CorePlanner("m") { _, _ -> """{"error":"core unavailable"}""" }.plan(doubleArrayOf(), null))
        assertNull(CorePlanner("m") { _, _ -> throw UnsatisfiedLinkError() }.plan(doubleArrayOf(), null))
        assertTrue(CorePlanner("m") { _, _ -> "junk" }.plan(doubleArrayOf(), null) == null)
    }
}
