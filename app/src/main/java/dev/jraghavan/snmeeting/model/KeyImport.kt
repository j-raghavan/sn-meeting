package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/**
 * F24: Settings → Keys → "Import key file…". The core finds, checks and copies the file; this
 * only reads its replies and words them. No key ever reaches this side (INV2). JVM-tested.
 */
object KeyImport {
    data class KeyFile(val path: String, val name: String, val folder: String) {
        /** "copilot-key-openai.txt · Download" (no folder for a file at the top). */
        val label: String get() = if (folder.isEmpty()) name else "$name · $folder"
    }

    sealed class Reply {
        data class Imported(val provider: String, val replaced: Boolean) : Reply()
        data class Exists(val provider: String) : Reply()
    }

    const val NOTE = "Copy your copilot-key file (for example copilot-key-openai.txt) to Download or Documents on this tablet, then import it. Meetings keeps it in MyStyle/Meetings/keys and reads MyStyle/SnCopilot too."
    const val NONE = "There's no copilot-key file in Download, Documents, INBOX or EXPORT, or at the top of this tablet's storage. Copy one there first, then try again."

    fun files(o: JSONObject): List<KeyFile> = o.arr("files").mapObj { f ->
        val path = f.str("path") ?: return@mapObj null
        KeyFile(path, f.strOr("name", path.substringAfterLast('/')), f.strOr("folder", ""))
    }

    fun reply(o: JSONObject): Reply? {
        o.str("imported")?.let { return Reply.Imported(it, o.bool("replaced") ?: false) }
        o.str("exists")?.let { return Reply.Exists(it) }
        return null
    }

    fun replaceQuestion(provider: String) =
        "Meetings already has a key for ${Format.providerName(provider)}. Replace it with this file?"

    fun done(r: Reply.Imported) =
        "${Format.providerName(r.provider)} key ${if (r.replaced) "replaced" else "imported"}."

    /** After an import: offer to delete the copy, so the key isn't left in Download. */
    fun deleteQuestion(r: Reply.Imported, file: KeyFile) =
        "${done(r)} Delete ${file.name} from ${file.folder.ifEmpty { "this tablet's storage" }} now that Meetings has its own copy?"
}
