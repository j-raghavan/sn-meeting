package dev.jraghavan.snmeeting.record

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.io.IOException

/** F15-FR1 (D14 revised 2026-10-09): the near-live files go through the core's MP4 writer. */
class Mp4FilesTest {
    private val log = mutableListOf<String>()
    private var openReply = """{"handle":7}"""
    private var writeReply = ""
    private var closeReply = """{"frames":2,"durationMs":128}"""

    private val port = object : Mp4Port {
        override fun open(path: String, rateHz: Int, channels: Int, asc: ByteArray, journal: String): String {
            log += "open $path $rateHz/$channels asc=${asc.joinToString(",")} journal=$journal"; return openReply
        }
        override fun write(handle: Long, frame: ByteArray): String { log += "write $handle ${frame.size}"; return writeReply }
        override fun close(handle: Long): String { log += "close $handle"; return closeReply }
        override fun abandon(handle: Long): String { log += "abandon $handle"; return """{"ok":true}""" }
    }

    private val config = AacConfig(16_000, 1, byteArrayOf(0x14, 0x08))
    private fun files(cfg: AacConfig? = config) = CoreAacFiles({ cfg }, port) { p -> if (p.endsWith("-01.m4a")) "$p.idx" else null }

    @Test fun framesGoToTheCoreAndCloseWritesTheIndex() {
        val f = files().open("/s/m-01.m4a")
        f.write(ByteArray(30), 0); f.write(ByteArray(31), 64_000)
        assertTrue(f.close())
        assertTrue("closing twice is harmless", f.close())
        assertEquals(listOf("open /s/m-01.m4a 16000/1 asc=20,8 journal=/s/m-01.m4a.idx", "write 7 30", "write 7 31", "close 7"), log)
    }

    @Test fun aChunkHasNoJournalAndAnEmptyFileIsLetGo() {
        val dir = java.nio.file.Files.createTempDirectory("mp4").toFile()
        val p = File(dir, "c-0001.m4a").apply { writeText("x") }.path
        val f = files().open(p)
        assertFalse(f.close())
        assertFalse(File(p).exists())
        assertEquals(listOf("open $p 16000/1 asc=20,8 journal=", "abandon 7"), log)
        dir.deleteRecursively()
    }

    @Test fun coreErrorsAreIOExceptions() {
        fun throws(block: () -> Unit) = try { block(); fail("no exception") } catch (e: IOException) { Unit }
        throws { files(cfg = null).open("/s/m-01.m4a") }
        openReply = """{"error":"open: denied"}"""
        throws { files().open("/s/m-01.m4a") }
        openReply = """{"handle":3}"""
        val f = files().open("/s/m-02.m4a")
        writeReply = """{"error":"write: disk full"}"""
        throws { f.write(ByteArray(3), 0) }
        writeReply = ""
        f.write(ByteArray(3), 0)
        closeReply = """{"error":"close: io"}"""
        throws { f.close() }
    }

    @Test fun aSegmentsJournalLivesBesideItsChunks() {
        val files = File("/data/app/files")
        assertEquals("/data/app/files/chunks/meeting-20261009-100000/seg-02.idx",
            Mp4Journal.forSegment(files, "/storage/EXPORT/sn-meeting/meeting-20261009-100000-02.m4a"))
        assertNull(Mp4Journal.forSegment(files, "/data/app/files/chunks/meeting-20261009-100000/c-0003.m4a"))
        assertNull(Mp4Journal.forSegment(files, "/x/meeting-20261009-100000-02.wav"))
    }
}
