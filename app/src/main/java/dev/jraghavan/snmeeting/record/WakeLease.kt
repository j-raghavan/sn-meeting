package dev.jraghavan.snmeeting.record

/**
 * F2-FR6: the partial wakelock for one recording. It is taken with a timeout, so a process that
 * dies without releasing it can't hold the CPU for ever, and renewed at every segment start;
 * [ExitGuard] releases it on every exit path (INV7).
 */
class WakeLease(private val wake: WakePort, private val timeoutMs: Long = TIMEOUT_MS) {
    var held = false
        private set

    fun begin(guard: ExitGuard) {
        if (held) return
        wake.acquire(timeoutMs); held = true
        guard.hold(NAME) { end() }
    }

    /** At each segment start: extends the timeout (the lock is not reference-counted). */
    fun renew() { if (held) wake.acquire(timeoutMs) }

    fun end() {
        if (!held) return
        held = false
        wake.release()
    }

    companion object {
        const val NAME = "wakelock"
        /** Two 15-minute segments plus slack. */
        const val TIMEOUT_MS = 35 * 60 * 1000L
    }
}
