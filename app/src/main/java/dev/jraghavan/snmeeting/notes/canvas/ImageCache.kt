// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/ImageCache.kt` (MIT
// License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md). The copy into the
// notes folder is the core's here (F21-FR11), so only the decoding came along.
package dev.jraghavan.snmeeting.notes.canvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import java.io.File

/**
 * The canvas's images (F21-FR11): each copy in the notes folder's `images/` decoded for
 * drawing, scaled down to at most [MAX_SIDE_PX] a side, the most recently drawn kept in a
 * [MAX_BYTES] cache. A missing or undecodable copy is remembered and draws as a crossed box.
 * The PDF export decodes off the UI thread, so every call is synchronized.
 */
class ImageCache : ImageSource {
    private val decoded = object : LruCache<String, Bitmap>(MAX_BYTES) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
    }
    private val unreadable = mutableSetOf<String>()

    /** The shown meeting's `images/` folder; null before the canvas opens. */
    var folder: File? = null
        @Synchronized get

        @Synchronized set(value) {
            if (value != field) unreadable.clear()
            field = value
        }

    @Synchronized
    override fun bitmap(image: DrawImage): Bitmap? {
        val file = File(folder ?: return null, image.file)
        return decoded.get(file.path) ?: if (file.path in unreadable) null else decode(file)
    }

    /** Forget a file that failed, so a copy that arrived later is read (an Undo of a delete). */
    @Synchronized
    fun retry() = unreadable.clear()

    private fun decode(file: File): Bitmap? {
        val bitmap = sizeOf(file)?.let { (w, h) ->
            val options = BitmapFactory.Options().apply { inSampleSize = sampleSize(maxOf(w, h)) }
            BitmapFactory.decodeFile(file.path, options)
        }
        if (bitmap == null) unreadable += file.path else decoded.put(file.path, bitmap)
        return bitmap
    }

    companion object {
        const val MAX_SIDE_PX = 1600
        const val MAX_BYTES = 32 * 1024 * 1024

        /** [file]'s size in pixels from its header alone; null when it isn't an image. */
        fun sizeOf(file: File): Pair<Int, Int>? {
            val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(file.path, options)
            return if (options.outWidth > 0 && options.outHeight > 0) options.outWidth to options.outHeight else null
        }

        /** The power-of-two step that brings [longestSide] down to [MAX_SIDE_PX] or less. */
        fun sampleSize(longestSide: Int): Int {
            var sample = 1
            while (longestSide / sample > MAX_SIDE_PX) sample *= 2
            return sample
        }
    }
}
