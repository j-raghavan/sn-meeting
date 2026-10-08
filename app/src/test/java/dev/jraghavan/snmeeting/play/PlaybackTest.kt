package dev.jraghavan.snmeeting.play

import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.MeetingView
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackTest {
    private val log = mutableListOf<String>()
    private var done: (() -> Unit)? = null
    private var pos = 0L
    private val unplayable = mutableSetOf<String>()
    private val port = object : PlayerPort {
        override fun open(path: String, onDone: () -> Unit): Boolean {
            if (path in unplayable) { log += "bad $path"; return false }
            log += "open $path"; done = onDone; return true
        }
        override fun start() { log += "start" }
        override fun pause() { log += "pause" }
        override fun positionMs() = pos
        override fun seekTo(ms: Long) { log += "seek $ms"; pos = ms }
        override fun release() { log += "release" }
    }
    private val list = SegmentPlaylist(listOf(SegmentPlaylist.Item("a", 900_000), SegmentPlaylist.Item("b", 900_000), SegmentPlaylist.Item("c", 60_000)))
    private var changes = 0
    private val p = Playback(list, port) { changes++ }

    @Test fun crossesSegmentBoundariesWithoutAnAction() { // F7-AC1
        assertEquals(1_860_000, list.totalMs)
        p.play()
        pos = 1_000; done!!()
        assertEquals(900_000 + 1_000, p.positionMs())
        done!!(); done!!()
        assertEquals(listOf("open a", "start", "release", "open b", "start", "release", "open c", "start", "release"), log)
        assertFalse(p.playing)
        assertEquals(0, p.positionMs()) // rewound, paused
    }

    @Test fun aSegmentWithoutAFileKeepsTheOthersOnTheirOwnPaths() { // N8
        val v = """{"meeting":{"id":"m","status":"recorded","segments":[{"startOffsetMs":0,"durationMs":1000},{"file":"m-02.m4a","startOffsetMs":1000,"durationMs":5000}]},
            "dir":"/d","segmentPaths":["/d/m-01.m4a","/d/m-02.m4a"]}"""
        val mv = CoreJson.obj(v, MeetingView::from).getOrNull()!!
        assertEquals(2, mv.meeting.segments.size)
        assertEquals(listOf(SegmentPlaylist.Item("/d/m-02.m4a", 5000)), SegmentPlaylist.of(mv).items)
    }

    @Test fun pauseAndResumeKeepThePlace() {
        p.play(); p.toggle(); pos = 5_000
        assertEquals(5_000, p.positionMs())
        p.toggle()
        assertEquals(listOf("open a", "start", "pause", "start"), log)
        assertTrue(p.playing)
        p.release()
        assertEquals("release", log.last())
    }

    @Test fun leavingTheScreenKeepsThePlaceInTheSegment() { // N10
        p.play(); done!!() // into b
        pos = 42_000
        p.release()
        assertEquals(900_000 + 42_000, p.positionMs())
        log.clear()
        p.play()
        assertEquals(listOf("open b", "seek 42000", "start"), log)
        // Played to the end: it starts over from the top.
        done!!(); done!!()
        log.clear(); p.play()
        assertEquals(listOf("open a", "start"), log)
    }

    @Test fun anUnplayableSegmentIsSkipped() {
        unplayable += "b"
        p.play(); done!!()
        assertEquals(listOf("open a", "start", "release", "bad b", "open c", "start"), log)
    }

    @Test fun nothingPlayableDoesNothing() {
        unplayable += listOf("a", "b", "c")
        p.play()
        assertFalse(p.playing)
        assertFalse(Playback(SegmentPlaylist(emptyList()), port) {}.canPlay)
    }

    @Test fun flaggedSegmentsAreLeftOut() {
        val json = """{"meeting":{"id":"m","segments":[{"file":"m-01.m4a","startOffsetMs":0,"durationMs":1000},
            {"file":"m-02.m4a","startOffsetMs":1000,"durationMs":0,"flag":"missing"},{"file":"m-03.m4a","startOffsetMs":2000,"durationMs":null}]},
            "segmentPaths":["/d/m-01.m4a","/d/m-02.m4a","/d/m-03.m4a"]}"""
        val v = CoreJson.obj(json, MeetingView::from).getOrNull()!!
        assertEquals(listOf(SegmentPlaylist.Item("/d/m-01.m4a", 1000), SegmentPlaylist.Item("/d/m-03.m4a", 0)), SegmentPlaylist.of(v).items)
    }
}
