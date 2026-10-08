package dev.jraghavan.snmeeting

import android.app.Activity

/**
 * Drops results that arrive after their screen is gone or was rebuilt: every async callback
 * goes through [deliver], which runs it only while [alive] and while the generation it was
 * issued in is still current.
 */
class ScreenScope(private val alive: () -> Boolean) {
    constructor(activity: Activity) : this({ !activity.isDestroyed })

    private var generation = 0

    /** Call on a re-render that makes in-flight results for the old views meaningless. */
    fun invalidate() { generation++ }

    fun <T> deliver(f: (T) -> Unit): (T) -> Unit {
        val g = generation
        return { v -> if (alive() && g == generation) f(v) }
    }

    /** A short core call on [CoreJobs]; [done] only while this screen still wants it. */
    fun core(work: () -> String, done: (String) -> Unit) = CoreJobs.run(work, deliver(done))
}

/**
 * At most one refresh per [minGapMs] (NFR2, F6-FR4). A request inside the gap is deferred to its
 * end; any number of requests in the meantime collapse into that one.
 */
class RefreshGate(
    private val now: () -> Long,
    private val schedule: (delayMs: Long, r: () -> Unit) -> () -> Unit,
    private val minGapMs: Long = 1000,
    private val refresh: () -> Unit,
) {
    private var last = Long.MIN_VALUE / 2
    private var pending: (() -> Unit)? = null

    fun request() {
        if (pending != null) return
        val wait = last + minGapMs - now()
        if (wait <= 0) return fire()
        pending = schedule(wait) { pending = null; fire() }
    }

    /** The screen stopped: drop a deferred refresh. */
    fun stop() {
        pending?.invoke(); pending = null
    }

    private fun fire() {
        last = now()
        refresh()
    }
}
