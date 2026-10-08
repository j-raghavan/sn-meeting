package dev.jraghavan.snmeeting.model

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
