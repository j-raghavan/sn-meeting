package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.model.Begun
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SegmentAdded
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SegmentScheduleTest { // F15-FR2
    private val usb = SegmentSchedule(44_100, 900_000)

    @Test fun segmentsEndAtTheFirstFrameBoundaryAfterExactlyThePlan() {
        // 15 min at 44.1 kHz is 39 690 000 samples = 38 759.77 frames: the boundary is frame 38 760.
        assertEquals(0L, usb.startFrame(0))
        assertEquals(38_760L, usb.startFrame(1))
        assertEquals(77_520L, usb.startFrame(2)) // 79 380 000 / 1024 = 77 519.53 → 77 520, from the first sample
        assertEquals(900_005L, usb.startOffsetMs(1)) // within one frame (23 ms) of the plan
        assertEquals(1_800_000L, SegmentSchedule(16_000, 900_000).startOffsetMs(2)) // 16 kHz divides evenly
    }

    @Test fun everyBoundaryIsGaplessAndNeverDrifts() { // F15-AC7: startOffset[i+1] = startOffset[i] + duration[i] ± one frame
        val frameMs = framesToMs(1, 44_100) + 1
        for (k in 0 until 12) {
            val duration = framesToMs(usb.startFrame(k + 1) - usb.startFrame(k), 44_100)
            val gap = usb.startOffsetMs(k + 1) - (usb.startOffsetMs(k) + duration)
            assertTrue("gap $gap at $k", gap in 0..frameMs)
            assertTrue(usb.startOffsetMs(k + 1) - (k + 1) * 900_000L in 0..frameMs) // 3 h: no drift
        }
    }

    @Test fun framesBelongToTheirSegment() {
        assertEquals(0, usb.segmentOf(0))
        assertEquals(0, usb.segmentOf(38_759))
        assertEquals(1, usb.segmentOf(38_760))
        assertEquals(2, usb.segmentOf(77_520))
        assertEquals(1, usb.segmentOf(77_519))
    }

    @Test fun theRollerApproachesRollsAndReportsAnUnarmedLimitOnce() {
        val s = SegmentSchedule(10_240, 10_000) // 100 ms frames: segments of exactly 100 frames
        val r = SegmentRoller(s, approachFrames = 20)
        val steps = (0L until 100).map { r.before(it) }
        assertEquals(SegmentRoller.Step.Approaching, steps[80])
        assertEquals(99, steps.count { it == SegmentRoller.Step.Write })
        r.arm("/s/m-02.m4a")
        assertEquals(SegmentRoller.Step.Roll(1, "/s/m-02.m4a"), r.before(100))
        assertEquals(1, r.segment)
        // Not armed at the next boundary (no room): the limit once, then the current file runs on.
        (101L until 200).forEach { r.before(it) }
        assertEquals(SegmentRoller.Step.Limit, r.before(200))
        assertEquals(SegmentRoller.Step.Write, r.before(201))
        r.arm("/s/m-03.m4a") // armed late: the roll happens at the next frame
        assertEquals(SegmentRoller.Step.Roll(2, "/s/m-03.m4a"), r.before(202))
        // Stopping: frames past the boundary stay in the current file.
        (203L until 300).forEach { r.before(it) }
        r.arm("/s/m-04.m4a"); r.stopping = true
        assertEquals(SegmentRoller.Step.Write, r.before(300))
    }

    // SegmentedRecorder with a time-driven recorder (D14): started with the plan's length, the
    // offsets it reports are used as they are.
    private val log = mutableListOf<String>()
    private val id = "meeting-20261009-100000"
    private val first = "/s/EXPORT/sn-meeting/$id-01.m4a"
    private lateinit var events: RecorderEvents
    private var now = 1_000L
    private val core = object : RecorderCore {
        override fun begin(startedAtMs: Long, micJson: String, audioJson: String, done: (CoreResult<Begun>) -> Unit) =
            done(CoreResult.Ok(Begun(id, first, 900_000, 13_499_997)))
        override fun addSegment(id: String, path: String, startOffsetMs: Long, done: (CoreResult<SegmentAdded>) -> Unit) {
            log += "add ${path.substringAfterLast('-')} @$startOffsetMs"; done(CoreResult.Ok(SegmentAdded(1, path)))
        }
        override fun closeSegment(id: String, index: Int, path: String, measure: Boolean, fallbackMs: Long) {
            log += "close $index measure=$measure ~$fallbackMs"
        }
        override fun finish(id: String, durationMs: Long, note: String, discard: Boolean, done: () -> Unit) { log += "finish"; done() }
    }
    private val timed = object : RecorderPort {
        override val timeDriven = true
        override fun accepts(spec: AudioSpec, source: InputSource, deviceId: Int) = true
        override fun start(spec: AudioSpec, source: InputSource, deviceId: Int, path: String, maxFileBytes: Long, maxDurationMs: Int, events: RecorderEvents) {
            log += "start ${path.substringAfterLast('-')} bytes=$maxFileBytes ms=$maxDurationMs"; this@SegmentScheduleTest.events = events
        }
        override fun setNext(path: String): Boolean { log += "next ${path.substringAfterLast('-')}"; return true }
        override fun stop(): Boolean { log += "stop"; return true }
    }
    private val storage = object : StoragePort {
        override fun usableBytes(path: String) = 10_000_000_000L
        override fun delete(path: String) {}
    }
    private val clock = object : ClockPort { override fun elapsedMs() = now; override fun wallMs() = 0L }

    @Test fun aTimeDrivenRecorderGetsThePlanAndItsOffsetsAreKept() {
        val sr = SegmentedRecorder(core, timed, storage, clock, object : SegmentedRecorder.Listener {
            override fun onRecording(id: String, startElapsedMs: Long) {}
            override fun onSegmentStarted(index: Int) { log += "segment $index" }
            override fun onEnded(id: String?, message: String) { log += "ended $message" }
        })
        sr.start(MicOption(MicKind.USB, "USB", 7), 7, InputSource.MIC, listOf(AudioSpec.USB))
        // The plan's length exactly, and its size cap only as a runaway guard.
        assertEquals("start 01.m4a bytes=13499997 ms=900000", log[0])
        events.onApproachingLimit()
        now += 912_345 // the elapsed time it is heard at doesn't matter
        events.onNextFileStartedAt(900_005)
        assertEquals(listOf("next 02.m4a", "close 0 measure=true ~900005", "add 02.m4a @900005", "segment 1"), log.subList(2, 6))
        // A next file never armed is ignored, as with MediaRecorder.
        log.clear()
        events.onNextFileStartedAt(1_800_010)
        assertEquals(emptyList<String>(), log)
    }
}
