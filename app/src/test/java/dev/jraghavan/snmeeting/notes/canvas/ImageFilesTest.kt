package dev.jraghavan.snmeeting.notes.canvas

import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.nio.file.Files

class ImageFilesTest { // F21-FR11: the Inkwell file list's images, newest first
    @Test fun imagesUnderTheRootsNewestFirst() {
        val root = Files.createTempDirectory("storage").toFile()
        fun put(path: String, at: Long) = File(root, path).apply { parentFile.mkdirs(); writeText("x"); setLastModified(at) }
        put("Pictures/old.png", 1_000)
        put("Download/new.JPEG", 9_000)
        put("EXPORT/a/b/deep.webp", 5_000)
        put("EXPORT/a/b/c/too-deep.png", 8_000)
        put("Pictures/notes.txt", 7_000)
        put("Music/song.png", 9_500)
        Files.createSymbolicLink(File(root, "Pictures/link.png").toPath(), File(root, "Pictures/old.png").toPath())
        assertEquals(listOf("new.JPEG", "deep.webp", "old.png"), ImageFiles.list(root).map { it.name })
        assertEquals(listOf("new.JPEG"), ImageFiles.list(root, limit = 1).map { it.name })
        root.deleteRecursively()
    }
}
