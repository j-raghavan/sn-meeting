package dev.jraghavan.snmeeting.record

/**
 * F2-FR3 / INV1: what a routing callback means, decided without ever throwing. Unplugging a
 * USB mic leaves the MediaRecorder in a state where getRoutedDevice() throws
 * IllegalStateException on the recorder thread; that, or any failure to read the input in use,
 * is the pinned mic gone. One MediaRecorder, one watch: [close] before stop()/release() turns
 * every later callback into a no-op that never touches the recorder.
 */
class RoutingWatch(private val pinnedId: Int, private val readRouted: () -> Int?) {
    enum class Verdict { CONTINUE, MIC_LOST, IGNORED }

    @Volatile var live = true
        private set

    fun close() { live = false }

    /** null: not routed yet, the routing callback reports it when it is (S14). Mic loss is final. */
    fun check(): Verdict {
        if (!live) return Verdict.IGNORED
        val routed = try {
            readRouted()
        } catch (t: Throwable) {
            live = false
            return Verdict.MIC_LOST
        }
        if (!Pin.routedAway(pinnedId, routed)) return Verdict.CONTINUE
        live = false
        return Verdict.MIC_LOST
    }
}

/**
 * Runs a platform callback for [events]: a throw is reported as a fault (the recording ends as a
 * recorder error) and never escapes onto the recorder thread, not even one from the report.
 */
inline fun guarded(events: RecorderEvents, where: String, block: () -> Unit) {
    try {
        block()
    } catch (t: Throwable) {
        try { events.onFault(where, t) } catch (_: Throwable) {}
    }
}
