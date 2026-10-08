package dev.jraghavan.snmeeting.play

import dev.jraghavan.snmeeting.model.MeetingView

/** F7-FR1: a meeting's segments as one continuous recording. */
class SegmentPlaylist(val items: List<Item>) {
    data class Item(val path: String, val durationMs: Long)

    val totalMs: Long = items.sumOf { it.durationMs }

    /** Where segment [index] starts in the whole recording. */
    fun offsetOf(index: Int): Long = items.take(index).sumOf { it.durationMs }

    companion object {
        /**
         * The segments that have audio: a missing or empty one (flagged by the core), or one with
         * no file, is skipped. Paths are the core's, one per segment, so they line up (N8).
         */
        fun of(v: MeetingView): SegmentPlaylist = SegmentPlaylist(
            v.meeting.segments.zip(v.segmentPaths)
                .filter { (s, _) -> s.flag == null && s.file != null }
                .map { (s, path) -> Item(path, s.durationMs ?: 0) },
        )
    }
}

/** One MediaPlayer at a time (DEVICE-UNVERIFIED adapter: MediaPlayerPort). Main thread. */
interface PlayerPort {
    /** Prepares [path]; false if it can't be played. [onDone] fires when it plays to the end. */
    fun open(path: String, onDone: () -> Unit): Boolean
    fun start()
    fun pause()
    fun positionMs(): Long
    fun release()
}

/**
 * Play/pause across segment boundaries with no user action (F7-AC1). A segment that can't be
 * opened is skipped. At the end it rewinds to the start, paused.
 */
class Playback(private val list: SegmentPlaylist, private val port: PlayerPort, private val onChange: () -> Unit) {
    var playing = false
        private set
    private var index = 0
    private var opened = false

    val canPlay get() = list.items.isNotEmpty()

    fun toggle() = if (playing) pause() else play()

    fun play() {
        if (playing || !canPlay) return
        if (!opened && !openFrom(index)) return finished()
        port.start(); playing = true
        onChange()
    }

    fun pause() {
        if (!playing) return
        port.pause(); playing = false
        onChange()
    }

    /** Elapsed time in the whole recording. */
    fun positionMs(): Long = list.offsetOf(index) + if (opened) port.positionMs() else 0

    /** Leaving the screen. */
    fun release() {
        if (opened) port.release()
        opened = false; playing = false
    }

    /** Opens the first playable segment from [from]; false when none is left. */
    private fun openFrom(from: Int): Boolean {
        for (i in from until list.items.size) {
            if (opened) { port.release(); opened = false }
            if (port.open(list.items[i].path) { next() }) {
                index = i; opened = true
                return true
            }
        }
        return false
    }

    private fun next() {
        if (openFrom(index + 1)) {
            port.start()
            onChange()
        } else {
            finished()
        }
    }

    private fun finished() {
        release()
        index = 0
        onChange()
    }
}
