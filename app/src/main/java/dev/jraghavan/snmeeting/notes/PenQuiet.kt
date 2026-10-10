package dev.jraghavan.snmeeting.notes

/**
 * F17-FR11 / R14: while the pen writes, the firmware paints its live ink over the panel, and a
 * partial refresh elsewhere (the timer, the strip) may flash or tear it. Such redraws wait
 * until the pen lifts: each kind keeps only its latest request, and they all run on lift (at
 * most a stroke's length later, under a second for writing). With the pen up they run at once.
 */
class PenQuiet {
    private val waiting = LinkedHashMap<String, () -> Unit>()
    var penDown = false
        private set

    /** Run [redraw] now, or when the pen lifts; a later request of the same [kind] replaces it. */
    fun redraw(kind: String, redraw: () -> Unit) {
        if (!penDown) return redraw()
        waiting[kind] = redraw
    }

    fun down() { penDown = true }

    fun up() {
        penDown = false
        val all = waiting.values.toList()
        waiting.clear()
        all.forEach { it() }
    }

    val pending: Int get() = waiting.size
}
