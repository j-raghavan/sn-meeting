package dev.jraghavan.snmeeting.record

/**
 * S15: Bluetooth profile proxies, which arrive asynchronously, held by profile until [close]
 * closes each once. A proxy arriving after close is closed at once and reported to no one, so
 * a screen or service that has gone never leaks a proxy or hears from it. Thread-safe: the
 * proxies arrive on the main thread, close may run on the recorder's.
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
        val changed = synchronized(this) { !closed && held.remove(profile) != null }
        if (changed) onChange()
    }

    fun close() {
        val all = synchronized(this) {
            closed = true
            held.toMap().also { held.clear() }
        }
        all.forEach { (profile, p) -> closeProxy(profile, p) }
    }
}
