package dev.jraghavan.snmeeting.record

/**
 * INV7: every acquired platform resource is released on every exit path, exactly once, in
 * reverse order of acquisition. A release that throws never stops the others.
 */
class ExitGuard(private val log: (String) -> Unit = {}) {
    private val held = ArrayDeque<Pair<String, () -> Unit>>()

    val names: List<String> get() = held.map { it.first }

    fun hold(name: String, release: () -> Unit) {
        held.addLast(name to release)
    }

    /** Releases one early (e.g. SCO after the recorder stops); no-op if not held. */
    fun release(name: String) {
        val i = held.indexOfLast { it.first == name }
        if (i >= 0) runRelease(held.removeAt(i))
    }

    fun releaseAll() {
        while (held.isNotEmpty()) runRelease(held.removeLast())
    }

    private fun runRelease(r: Pair<String, () -> Unit>) {
        try { r.second() } catch (e: Exception) { log("release ${r.first} failed: ${e.javaClass.simpleName}") }
    }
}
