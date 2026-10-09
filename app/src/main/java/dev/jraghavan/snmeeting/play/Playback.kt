package dev.jraghavan.snmeeting.play

import dev.jraghavan.snmeeting.model.MeetingView

/**
 * F7-FR1: a meeting's segments as one continuous recording. A segment's stored length may be 0
 * (not measured yet, D3); it still plays, and [Playback] takes the player's own length for it.
 */
class SegmentPlaylist(val items: List<Item>) {
    data class Item(val path: String, val durationMs: Long)

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
    /** The open file's length, 0 when the player doesn't know it. */
    fun durationMs(): Long
    fun seekTo(ms: Long)
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
    /** Where in segment [index] to carry on after [release] (N10: leaving the screen). */
    private var resumeAt = 0L
    /** Each segment's length: the stored one, or the player's once opened if none was stored. */
    private val lengths = LongArray(list.items.size) { list.items[it].durationMs }

    val canPlay get() = list.items.isNotEmpty()

    fun toggle() = if (playing) pause() else play()

    fun play() {
        if (playing || !canPlay) return
        if (!opened) {
            val (at, from) = resumeAt to index
            if (!openFrom(index)) return finished()
            if (at > 0 && index == from) port.seekTo(at) // not into the next one if it was skipped
        }
        port.start(); playing = true
        onChange()
    }

    fun pause() {
        if (!playing) return
        port.pause(); playing = false
        onChange()
    }

    /** Elapsed time in the whole recording. */
    fun positionMs(): Long = offsetOf(index) + if (opened) port.positionMs() else resumeAt

    /** The whole recording's length, as far as it is known. */
    fun totalMs(): Long = lengths.sum()

    private fun offsetOf(i: Int): Long = (0 until i).sumOf { lengths[it] }

    /** Leaving the screen: the place is kept, and the next play carries on from it. */
    fun release() {
        if (opened) { resumeAt = port.positionMs(); port.release() }
        opened = false; playing = false
    }

    /** Opens the first playable segment from [from]; false when none is left. */
    private fun openFrom(from: Int): Boolean {
        for (i in from until list.items.size) {
            if (opened) { port.release(); opened = false }
            if (port.open(list.items[i].path) { next() }) {
                index = i; opened = true; resumeAt = 0
                if (lengths[i] <= 0) lengths[i] = port.durationMs().coerceAtLeast(0)
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
        index = 0; resumeAt = 0
        onChange()
    }
}
