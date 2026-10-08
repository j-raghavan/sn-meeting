package dev.jraghavan.snmeeting

import org.junit.Assert.assertEquals
import org.junit.Test

class ScreenScopeTest {
    @Test fun resultsForAGoneOrRebuiltScreenAreDropped() {
        var alive = true
        val scope = ScreenScope { alive }
        val got = mutableListOf<String>()
        val first = scope.deliver<String> { got += it }
        scope.invalidate()
        val second = scope.deliver<String> { got += it }
        first("old"); second("new")
        alive = false
        second("late")
        assertEquals(listOf("new"), got)
    }
}

class RefreshGateTest {
    private var now = 10_000L
    private val scheduled = mutableListOf<Pair<Long, () -> Unit>>()
    private var cancelled = 0
    private var refreshes = 0
    private val gate = RefreshGate({ now }, { d, r -> scheduled += d to r; { cancelled++ } }) { refreshes++ }

    @Test fun atMostOncePerSecondWithRequestsCollapsed() {
        gate.request()
        assertEquals(1, refreshes)
        now += 300; gate.request(); gate.request(); gate.request()
        assertEquals(1, refreshes)
        assertEquals(listOf(700L), scheduled.map { it.first })
        now += 700; scheduled.removeAt(0).second()
        assertEquals(2, refreshes)
        now += 1500; gate.request()
        assertEquals(3, refreshes)
    }

    @Test fun stopDropsTheDeferredRefresh() {
        gate.request(); now += 10; gate.request(); gate.stop()
        assertEquals(1, cancelled)
        gate.request() // a new request after stop schedules again
        assertEquals(2, scheduled.size)
    }
}
