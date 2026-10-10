package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** F20: how tags read in the shell. The core stores and normalises them (F20-FR2). */
object Tags {
    /** `#local-ai`. */
    fun shown(name: String): String = "#$name"

    /** F20-FR4: the question a tap on a tag pill asks, once. */
    fun removeQuestion(name: String): String = "Remove ${shown(name)} from this meeting? A new summary won't add it back."

    /** F20-FR6: how many tags a Home row shows. */
    const val ON_HOME = 3

    /** F20-FR6: a Home row's tag line, "#gpu  #vram  #local-ai" (the core orders them). */
    fun line(tags: List<String>): String = tags.take(ON_HOME).joinToString("  ") { shown(it) }

    /** The add-a-tag dialog's hint. */
    const val ADD_HINT = "A word or two, like local-ai"

    /** `nativeAddTag` / `nativeRemoveTag`: `{tags: [{name, source}]}`. */
    fun names(o: JSONObject): List<String> = o.arr("tags").mapObj { it.str("name") }
}
