package dev.jraghavan.snmeeting.play

import android.media.MediaPlayer

/** DEVICE-UNVERIFIED: PlayerPort over MediaPlayer, one segment file at a time. */
class MediaPlayerPort : PlayerPort {
    private var mp: MediaPlayer? = null

    override fun open(path: String, onDone: () -> Unit): Boolean {
        release()
        val m = MediaPlayer()
        return try {
            m.setDataSource(path)
            m.setOnCompletionListener { onDone() }
            m.setOnErrorListener { _, _, _ -> onDone(); true } // a broken file ends; the next one plays
            m.prepare()
            mp = m
            true
        } catch (e: Exception) {
            m.release()
            false
        }
    }

    override fun start() { mp?.start() }
    override fun pause() { mp?.pause() }
    override fun positionMs(): Long = mp?.currentPosition?.toLong() ?: 0
    override fun seekTo(ms: Long) { mp?.seekTo(ms.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()) }

    override fun release() {
        mp?.release()
        mp = null
    }
}
