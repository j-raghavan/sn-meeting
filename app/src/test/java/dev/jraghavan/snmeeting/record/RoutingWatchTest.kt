package dev.jraghavan.snmeeting.record

import dev.jraghavan.snmeeting.record.RoutingWatch.Verdict
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** F2-FR3 / INV1: what a routing callback means, decided without ever throwing. */
class RoutingWatchTest {
    @Test fun aRoutedDeviceThatCantBeReadIsMicLoss() { // the on-device crash: native threw
        val w = RoutingWatch(7) { throw IllegalStateException() }
        assertEquals(Verdict.MIC_LOST, w.check())
    }

    @Test fun anyFailureToReadIsMicLoss() {
        assertEquals(Verdict.MIC_LOST, RoutingWatch(7) { throw RuntimeException("dead") }.check())
        assertEquals(Verdict.MIC_LOST, RoutingWatch(7) { throw StackOverflowError() }.check())
    }

    @Test fun routedToThePinnedInputContinues() {
        assertEquals(Verdict.CONTINUE, RoutingWatch(7) { 7 }.check())
    }

    @Test fun routedToAnotherInputIsMicLoss() { // INV1: never another mic
        assertEquals(Verdict.MIC_LOST, RoutingWatch(7) { 3 }.check())
    }

    @Test fun notRoutedYetContinues() { // S14: the routing callback reports it when it is
        assertEquals(Verdict.CONTINUE, RoutingWatch(7) { null }.check())
    }

    @Test fun aCallbackAfterStopIsIgnoredWithoutTouchingTheRecorder() {
        var reads = 0
        val w = RoutingWatch(7) { reads++; throw IllegalStateException() }
        assertTrue(w.live)
        w.close()
        assertFalse(w.live)
        assertEquals(Verdict.IGNORED, w.check())
        assertEquals(0, reads)
    }

    @Test fun micLossIsReportedOnce() {
        var routed: Int? = 3
        val w = RoutingWatch(7) { routed }
        assertEquals(Verdict.MIC_LOST, w.check())
        routed = 7
        assertEquals(Verdict.IGNORED, w.check())
    }

    @Test fun guardedReportsAThrowAsAFault() {
        val seen = mutableListOf<String>()
        val events = object : RecorderEvents {
            override fun onApproachingLimit() {}
            override fun onNextFileStarted() {}
            override fun onLimitReached() {}
            override fun onError(what: Int) {}
            override fun onRoutedTo(deviceId: Int?) {}
            override fun onMicLost() {}
            override fun onFault(where: String, t: Throwable) { seen += "$where ${t.javaClass.simpleName}"; throw IllegalStateException("again") }
        }
        guarded(events, "routing") { throw IllegalStateException() } // nothing escapes, even a second throw
        guarded(events, "info") { seen += "ran" }
        assertEquals(listOf("routing IllegalStateException", "ran"), seen)
    }
}
