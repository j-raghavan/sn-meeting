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

/** F20-FR10 / OQ31: "Tag older meetings…": the dry run's count and estimate, asked first. */
data class TagOlder(val meetings: Int, val estUsd: Double?, val provider: String, val model: String) {
    /** The question to confirm, or null when there is nothing to tag. */
    fun question(): String? {
        if (meetings == 0) return null
        val cost = estUsd?.let { "about ${Spend.money(it)}" } ?: "a cost this model has no rate for yet"
        return "${Format.plural(meetings, "meeting")} without tags will each get one small call to " +
            "${Format.providerName(provider)} ($model) on its stored summary and transcript: $cost."
    }

    companion object {
        const val NOTE = "Meetings summarised before tags existed can be tagged once, on request."

        fun from(o: JSONObject): TagOlder = TagOlder(
            o.int("meetings") ?: 0, if (o.isNull("estUsd")) null else o.optDouble("estUsd"),
            o.strOr("provider", ""), o.strOr("model", ""),
        )

        /** `{tagged, failed, error}` as one line. */
        fun result(o: JSONObject): String {
            val tagged = o.int("tagged") ?: 0
            val failed = o.int("failed") ?: 0
            return buildString {
                append("Tagged ${Format.plural(tagged, "meeting")}")
                if (failed > 0) append("  ·  ${failed} couldn't be tagged" + (o.str("error")?.let { ": $it" } ?: ""))
            }
        }
    }
}
