package dev.jraghavan.snmeeting.record

/**
 * S15 / F2-FR2: Bluetooth profile proxies, which arrive asynchronously, held by profile until
 * [close] closes each once. A proxy arriving after close is closed at once and reported to no
 * one, and one whose service disconnects is closed as it is dropped, so every proxy that was
 * ever handed over is closed exactly once and a screen or service never leaks its binding.
 * Thread-safe: the proxies arrive on the main thread, close may run on the recorder's.
 */
class ProxySlots<P : Any>(private val closeProxy: (profile: Int, proxy: P) -> Unit, private val onChange: () -> Unit) {
    private val held = mutableMapOf<Int, P>()
    private var closed = false

    @Synchronized operator fun get(profile: Int): P? = held[profile]

    fun connected(profile: Int, proxy: P) {
        val stale = synchronized(this) {
            if (closed) proxy else held.put(profile, proxy)?.takeIf { it !== proxy }
        }
        stale?.let { closeProxy(profile, it) }
        if (stale !== proxy) onChange()
    }

    fun disconnected(profile: Int) {
        val gone = synchronized(this) { if (closed) null else held.remove(profile) }
        gone?.let { closeProxy(profile, it); onChange() }
    }

    fun close() {
        val all = synchronized(this) {
            closed = true
            held.toMap().also { held.clear() }
        }
        all.forEach { (profile, p) -> closeProxy(profile, p) }
    }
}
