package dev.jraghavan.snmeeting.record

/** What the service is doing, as the UI sees it. */
sealed class RecState {
    /** [message]: how the last recording ended, shown until the next one starts (null at launch). */
    data class Idle(val message: String?, val lastId: String?) : RecState()
    data class Starting(val mic: String) : RecState()
    data class Recording(val id: String, val mic: String, val startElapsedMs: Long) : RecState()
    object Stopping : RecState()

    val busy get() = this !is Idle
}

/**
 * In-process service → UI state (same process, so no broadcast or binder). The service
 * publishes on transitions only (a few per meeting); the UI ticks its own 1 Hz timer from
 * [RecState.Recording.startElapsedMs] (NFR2: at most one redraw a second). [post] marshals to
 * the main thread, where listeners are added, removed and called.
 */
class RecordingHub(private val post: (() -> Unit) -> Unit) {
    @Volatile var state: RecState = RecState.Idle(null, null)
        private set
    private val listeners = LinkedHashSet<(RecState) -> Unit>()

    fun publish(s: RecState) {
        state = s
        post { listeners.toList().forEach { it(s) } }
    }

    /** Main thread only. Delivers the current state at once; the returned function unsubscribes. */
    fun observe(l: (RecState) -> Unit): () -> Unit {
        listeners += l
        l(state)
        return { listeners -= l }
    }
}
