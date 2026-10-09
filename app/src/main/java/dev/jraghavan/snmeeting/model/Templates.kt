package dev.jraghavan.snmeeting.model

import java.text.Normalizer
import org.json.JSONArray
import org.json.JSONObject

/** Where a template's effective version comes from (F11-FR3); the list's chip. */
enum class TemplateOrigin(val label: String) {
    BUILTIN("Built-in"), CUSTOMISED("Customised"), YOURS("Yours");

    companion object { fun of(s: String?) = entries.firstOrNull { it.name.lowercase() == s } ?: YOURS }
}

/** One row of nativeListTemplates, and nativeGetTemplate / nativeSaveTemplate's reply. */
data class TemplateRow(
    val id: String, val name: String, val description: String,
    val vocabulary: List<String>, val sections: List<String>,
    val instructions: String, val layout: String,
    val origin: TemplateOrigin, val builtin: Boolean,
    /** "Your copy has an error on line N; using the built-in", or null. */
    val problem: String?,
    /** The user's file, when there is one. */
    val path: String?,
) {
    companion object {
        fun from(o: JSONObject): TemplateRow? { return TemplateRow(
            id = o.str("id") ?: return null,
            name = o.strOr("name", ""),
            description = o.strOr("description", ""),
            vocabulary = o.arr("vocabulary").strings(),
            sections = o.arr("sections").strings(),
            instructions = o.strOr("instructions", ""),
            layout = o.strOr("layout", ""),
            origin = TemplateOrigin.of(o.str("origin")),
            builtin = o.bool("builtin") ?: false,
            problem = o.str("problem"),
            path = o.str("path"),
        ) }
    }
}

/** F11-FR6: the meeting's template card and its picker. */
data class TemplateChoice(val current: TemplateRow?, val note: String?, val options: List<TemplateRow>) {
    companion object {
        /** A YOURS template whose file has an error can't be used by a job, so it isn't offered. */
        private fun usable(t: TemplateRow) = t.problem == null || t.origin != TemplateOrigin.YOURS

        /**
         * [templateId] is the meeting's (null until set or first transcribed: Settings'
         * [default] applies). A template that has gone falls back to General, with a note.
         */
        fun of(templateId: String?, default: String, rows: List<TemplateRow>): TemplateChoice {
            val options = rows.filter(::usable)
            val wanted = templateId ?: default
            val found = options.firstOrNull { it.id == wanted }
            if (found != null) return TemplateChoice(found, null, options)
            val general = options.firstOrNull { it.id == GENERAL }
            val note = if (templateId != null) "Its template is gone, so ${general?.name ?: "General meeting"} is used" else null
            return TemplateChoice(general, note, options)
        }

        const val GENERAL = "general"
    }
}

/** F11-FR5: what the editor sends to nativeSaveTemplate / nativePreviewTemplate. */
data class TemplateDraft(
    val id: String, val name: String, val description: String, val vocabulary: String,
    val sections: List<String>, val instructions: String, val layout: String,
) {
    fun json(): String = JSONObject()
        .put("id", id).put("name", name.trim()).put("description", description.trim())
        .put("vocabulary", JSONArray(vocabularyList()))
        .put("sections", JSONArray(sections))
        .put("instructions", instructions).put("layout", layout)
        .toString()

    /** "Priya, Arjun , Supernote" → [Priya, Arjun, Supernote]. */
    fun vocabularyList(): List<String> = vocabulary.split(',').map { it.trim() }.filter { it.isNotEmpty() }

    companion object {
        fun of(t: TemplateRow) = TemplateDraft(t.id, t.name, t.description, t.vocabulary.joinToString(", "), t.sections, t.instructions, t.layout)

        /** The core's refusal as the editor shows it under the field: "Line 12: …" when it has a line. */
        fun errorText(e: CoreResult.Err): String {
            val line = e.line ?: return e.message
            return if (e.message.contains("line $line")) e.message else "Line $line: ${e.message}"
        }

        /** A template id (kebab-case a-z 0-9, at most 64) from a name, unique among [taken]. */
        fun slug(name: String, taken: Collection<String>): String {
            val ascii = Normalizer.normalize(name, Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")
            val base = ascii.lowercase().replace(Regex("[^a-z0-9]+"), "-").trim('-').take(56).trim('-').ifEmpty { "my-template" }
            if (base !in taken) return base
            return (2..999).asSequence().map { "$base-$it" }.first { it !in taken }
        }
    }
}

/** F11-FR5: which of Reset / Duplicate / Delete a template offers. */
data class TemplateActions(val reset: Boolean, val delete: Boolean) {
    companion object {
        /** Reset for a built-in with a user file (customised, or a copy with an error); Delete for YOURS. */
        fun of(t: TemplateRow) = TemplateActions(reset = t.builtin && t.path != null, delete = t.origin == TemplateOrigin.YOURS)

        /** "Stand-up (copy)", kept within the 60-character name limit. */
        fun copyName(name: String): String {
            val suffix = " (copy)"
            return name.take(60 - suffix.length).trimEnd() + suffix
        }
    }
}
