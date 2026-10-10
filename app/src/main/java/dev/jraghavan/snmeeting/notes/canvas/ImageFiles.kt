package dev.jraghavan.snmeeting.notes.canvas

import java.io.File

/**
 * F21-FR11: the images Insert ▸ Image offers: `.png`, `.jpg`, `.jpeg` and `.webp` files under
 * shared storage's Pictures/, Download/, INBOX/, SCREENSHOT/ and EXPORT/ (a few folders deep),
 * most recent first. Ratta's plugin file selector isn't available to an APK, and
 * `ACTION_OPEN_DOCUMENT` on the Manta is unverified (R23). Links are not followed. JVM-tested.
 */
object ImageFiles {
    val ROOTS = listOf("Pictures", "Download", "INBOX", "SCREENSHOT", "EXPORT")
    private val EXTENSIONS = setOf("png", "jpg", "jpeg", "webp")
    const val MAX_DEPTH = 3
    const val LIMIT = 300

    fun isImage(f: File) = f.extension.lowercase() in EXTENSIONS

    /** The images under [storage]'s roots, newest first, at most [limit]. */
    fun list(storage: File, limit: Int = LIMIT): List<File> {
        val out = ArrayList<File>()
        fun walk(dir: File, depth: Int) {
            val kids = dir.listFiles() ?: return
            for (k in kids) {
                if (java.nio.file.Files.isSymbolicLink(k.toPath())) continue
                if (k.isDirectory) { if (depth < MAX_DEPTH && !k.name.startsWith(".")) walk(k, depth + 1) }
                else if (k.isFile && isImage(k)) out += k
            }
        }
        ROOTS.map { File(storage, it) }.filter { it.isDirectory }.forEach { walk(it, 1) }
        return out.sortedWith(compareByDescending<File> { it.lastModified() }.thenBy { it.path }).take(limit)
    }
}
