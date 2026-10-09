package dev.jraghavan.snmeeting.record

/**
 * The recorder thread's one guard: every post and platform callback on it runs through [run],
 * so a throw is logged ([log] gets where and the throwable, nothing else) and becomes an orderly
 * end ([onFault]: finalise the segment, finish as "recorder error") instead of a crash.
 */
class Safely(private val log: (String, Throwable) -> Unit, private val onFault: (Throwable) -> Unit) {
    fun run(where: String, block: () -> Unit) {
        try { block() } catch (t: Throwable) { fail(where, t) }
    }

    /** true if the orderly end ran; false if it threw too (the caller's last resort decides). */
    fun fail(where: String, t: Throwable): Boolean {
        quietly { log("$where failed", t) }
        return try {
            onFault(t); true
        } catch (t2: Throwable) {
            quietly { log("ending after a fault failed", t2) }
            false
        }
    }

    private inline fun quietly(block: () -> Unit) { try { block() } catch (_: Throwable) {} }
}
