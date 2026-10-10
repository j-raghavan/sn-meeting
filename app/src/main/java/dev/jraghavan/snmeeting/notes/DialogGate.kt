package dev.jraghavan.snmeeting.notes

/**
 * F17-FR14 (device finding 2026-10-09: a dialog on the first stroke took the window's focus,
 * the firmware pen lost its claim and ink "ghosted"): nothing opens over the Notes canvas while
 * the pen is in use. A dialog asked for with the pen down, or within [IDLE_MS] of it lifting,
 * waits until the canvas has been idle that long ([tick]), then opens; with the pen idle it
 * opens at once.
 */
class DialogGate(private val now: () -> Long, private val idleMs: Long = IDLE_MS) {
    private val waiting = ArrayDeque<() -> Unit>()
    private var penDown = false
    private var lastPen = Long.MIN_VALUE / 2

    fun pen(down: Boolean) {
        penDown = down
        lastPen = now()
    }

    fun idle(): Boolean = !penDown && now() - lastPen >= idleMs

    fun show(dialog: () -> Unit) {
        if (idle()) dialog() else waiting.addLast(dialog)
    }

    /** Opens what waited, once the canvas is idle. */
    fun tick() {
        if (!idle()) return
        while (waiting.isNotEmpty()) waiting.removeFirst()()
    }

    val pending: Int get() = waiting.size

    companion object {
        const val IDLE_MS = 1_500L
    }
}
