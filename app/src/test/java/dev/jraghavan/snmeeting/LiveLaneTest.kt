package dev.jraghavan.snmeeting

import dev.jraghavan.snmeeting.record.Cancel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveLaneTest { // F15-FR5 / FR7
    private val queue = ArrayDeque<() -> Unit>()
    private val timers = mutableListOf<Pair<Long, () -> Unit>>()
    private val replies = ArrayDeque<String>()
    private val passes = mutableListOf<String>()
    private val seen = mutableListOf<LiveReply>()
    private val lane = LiveLane(
        run = { queue += it },
        after = { ms, r -> val t = ms to r; timers += t; Cancel { timers -= t } },
        drain = { id -> passes += id; replies.removeFirstOrNull() ?: """{"state":"idle","done":0,"waiting":0}""" },
        drained = { _, r -> seen += r },
    )

    private fun runAll() { while (queue.isNotEmpty()) queue.removeFirst()() }

    @Test fun aKickSendsWhatIsPending() {
        replies += """{"state":"idle","done":2,"waiting":0}"""
        lane.kick("m")
        runAll()
        assertEquals(listOf("m"), passes)
        assertEquals(LiveReply("idle", 2, 0, null, null), seen.single())
    }

    @Test fun aKickDuringAPassRunsOneMorePassAfterIt() {
        lane.kick("m")
        lane.kick("m"); lane.kick("m") // two more chunks while the first pass is queued
        runAll()
        assertEquals(listOf("m", "m"), passes)
    }

    @Test fun aFailureWaitsTheBackoffAndKicksMeanwhileAreIgnored() { // F15-AC3: later chunks wait
        replies += """{"state":"retry","retryInMs":5000,"done":2,"waiting":4}"""
        replies += """{"state":"retry","retryInMs":15000,"done":0,"waiting":4}"""
        lane.kick("m"); runAll()
        assertEquals(5_000L, timers.single().first)
        assertTrue(lane.isBackingOff("m"))
        lane.kick("m"); runAll() // a new chunk: no pass before the back-off ends
        assertEquals(1, passes.size)
        timers.removeFirst().second(); runAll()
        assertEquals(15_000L, timers.single().first)
        timers.removeFirst().second(); runAll() // the network is back: it catches up
        assertEquals(3, passes.size)
        assertFalse(lane.isBackingOff("m"))
        assertEquals("idle", seen.last().state)
    }

    @Test fun pausedStoppedAndOffEndTheLaneUntilTheNextKick() {
        for (state in listOf("paused", "stopped", "off", "busy")) {
            replies += """{"state":"$state","done":0,"waiting":1,"reason":"Transcription paused: OpenAI rejected the key (401)"}"""
            lane.kick("m"); runAll()
            assertFalse(lane.isBackingOff("m"))
        }
        assertEquals("Transcription paused: OpenAI rejected the key (401)", seen.first().reason)
        assertEquals(4, passes.size)
    }

    @Test fun forgettingAMeetingCancelsItsRetry() {
        replies += """{"state":"retry","done":0,"waiting":1}""" // no wait named: the longest
        lane.kick("m"); runAll()
        assertEquals(LiveLane.RETRY_FALLBACK_MS, timers.single().first)
        lane.forget("m")
        assertTrue(timers.isEmpty())
        assertFalse(lane.isBackingOff("m"))
    }

    @Test fun aBrokenCoreOrJunkIsAnErrorThatEndsThePass() {
        val l = LiveLane({ it() }, { _, _ -> Cancel {} }, { throw UnsatisfiedLinkError("no core") }, { _, r -> seen += r })
        l.kick("m")
        assertEquals("error", seen.single().state)
        assertEquals("error", LiveReply.of("junk").state)
        assertEquals(LiveReply("error", 0, 0, null, "nativeInit has not run"), LiveReply.of("""{"error":"nativeInit has not run"}"""))
    }
}
