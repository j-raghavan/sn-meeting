package dev.jraghavan.snmeeting.model

import org.json.JSONObject

/** settings.mic over JNI: "auto", "usb:<name>" or "bt:<name>" (F2-FR8, F10-FR2). */
sealed class MicChoice {
    object Auto : MicChoice()
    data class Usb(val name: String) : MicChoice()
    data class Bluetooth(val name: String) : MicChoice()

    val wire: String get() = when (this) { Auto -> "auto"; is Usb -> "usb:$name"; is Bluetooth -> "bt:$name" }

    companion object {
        fun of(s: String?): MicChoice = when {
            s == null -> Auto
            s.startsWith("usb:") -> Usb(s.removePrefix("usb:"))
            s.startsWith("bt:") -> Bluetooth(s.removePrefix("bt:"))
            else -> Auto
        }
    }
}

/** A provider's key file as the core reports it (F4-FR5). Never holds the key. */
sealed class KeyStatus {
    abstract val file: String
    data class Found(override val file: String, val model: String, val transcribeModel: String?) : KeyStatus()
    /** missing, vault, invalid or unreadable; [detail] is the line to show. */
    data class Problem(val kind: String, override val file: String, val detail: String) : KeyStatus()

    val usable get() = this is Found

    companion object {
        fun from(o: JSONObject): KeyStatus = when (val s = o.str("status")) {
            "found" -> Found(o.strOr("file", ""), o.strOr("model", ""), o.str("transcribeModel"))
            else -> Problem(s ?: "missing", o.strOr("file", ""), o.strOr("detail", ""))
        }
    }
}

data class RoleChoice(val provider: String?, val model: String?)
data class Effective(val provider: String, val model: String?, val modelSource: String?)

/** nativeGetSettings / nativeSetSettings. */
data class SettingsView(
    val mic: MicChoice, val saveFolder: String, val previousFolders: List<String>,
    val transcribe: RoleChoice, val summarise: RoleChoice, val defaultTemplate: String,
    val noticeShown: Map<String, Boolean>, val segmentMsDebug: Long?,
    val origin: String, val effectiveTranscribe: Effective, val effectiveSummarise: Effective,
    val saveFolderPath: String, val effectiveDefaultTemplate: String, val keys: Map<String, KeyStatus>,
) {
    fun effective(role: String) = if (role == TRANSCRIBE) effectiveTranscribe else effectiveSummarise
    fun choice(role: String) = if (role == TRANSCRIBE) transcribe else summarise

    companion object {
        const val TRANSCRIBE = "transcribe"
        const val SUMMARISE = "summarise"
        val PROVIDERS = listOf("openai", "gemini", "anthropic")

        /** Anthropic takes no audio (Non-Goals), so it summarises only. */
        fun providersFor(role: String) = if (role == TRANSCRIBE) listOf("openai", "gemini") else PROVIDERS

        private fun role(o: JSONObject?) = RoleChoice(o?.str("provider"), o?.str("model"))
        private fun eff(o: JSONObject?) = Effective(o?.str("provider") ?: "openai", o?.str("model"), o?.str("modelSource"))

        fun from(o: JSONObject): SettingsView? {
            val s = o.obj("settings") ?: return null
            val e = o.obj("effective") ?: return null
            val n = s.obj("noticeShown")
            val k = o.obj("keys")
            val default = s.strOr("defaultTemplate", "general")
            return SettingsView(
                mic = MicChoice.of(s.str("mic")),
                saveFolder = s.strOr("saveFolder", ""),
                previousFolders = s.arr("previousFolders").strings(),
                transcribe = role(s.obj("transcribe")), summarise = role(s.obj("summarise")),
                defaultTemplate = default,
                noticeShown = PROVIDERS.associateWith { n?.bool(it) ?: false },
                segmentMsDebug = s.obj("debug")?.long("segmentMs"),
                origin = o.strOr("origin", ""),
                effectiveTranscribe = eff(e.obj("transcribe")), effectiveSummarise = eff(e.obj("summarise")),
                saveFolderPath = e.strOr("saveFolderPath", ""),
                effectiveDefaultTemplate = e.strOr("defaultTemplate", default),
                keys = PROVIDERS.mapNotNull { p -> k?.obj(p)?.let { p to KeyStatus.from(it) } }.toMap(),
            )
        }
    }
}

/** RFC 7396 patches the shell sends; built here so the JSON shape is unit-tested. */
object SettingsPatch {
    fun mic(c: MicChoice) = """{"mic":${CoreJson.quote(c.wire)}}"""
    fun saveFolder(rel: String) = """{"saveFolder":${CoreJson.quote(rel)}}"""
    fun provider(role: String, p: String) = """{"$role":{"provider":${CoreJson.quote(p)}}}"""
    /** Empty clears the override (JSON null removes the key). */
    fun model(role: String, m: String) = """{"$role":{"model":${if (m.isBlank()) "null" else CoreJson.quote(m.trim())}}}"""
    fun defaultTemplate(id: String) = """{"defaultTemplate":${CoreJson.quote(id)}}"""
    /** F2-AC3's hidden segment length; null returns to the standard plan. */
    fun segmentMs(ms: Long?) = """{"debug":{"segmentMs":${ms ?: "null"}}}"""
    fun noticeShown(providers: Collection<String>) =
        providers.joinToString(",", """{"noticeShown":{""", "}}") { "${CoreJson.quote(it)}:true" }
}

/** nativeTestConnection: `{ok, status, ms, provider, model, model_found, detail}`. */
data class TestResult(val ok: Boolean, val status: String?, val ms: Long?, val model: String?, val modelFound: Boolean?, val detail: String) {
    companion object {
        fun from(o: JSONObject) = TestResult(o.bool("ok") ?: false, o.str("status"), o.long("ms"), o.str("model"), o.bool("model_found"), o.strOr("detail", ""))
    }
}

/** F10-FR3: the save-folder presets; anything else is Custom. Paths are relative to internal storage. */
object SaveFolders {
    data class Preset(val path: String, val hint: String?)

    val PRESETS = listOf(
        Preset("EXPORT/sn-meeting", "Synced to your computer by Supernote Partner"),
        Preset("Document/Meetings", null),
    )

    fun isCustom(folder: String) = PRESETS.none { it.path == folder }

    /** "EXPORT/sn-meeting and Document/Old": where older meetings stay listed from. */
    fun previous(folders: List<String>): String? =
        if (folders.isEmpty()) null else "Meetings in ${folders.joinToString(" and ")} stay listed."
}

/** F10-FR4: one role's provider rows and the model that will run. */
object ProviderRows {
    data class Row(val id: String, val name: String, val hint: String, val selected: Boolean, val enabled: Boolean)

    /** A provider without a usable key can't be chosen; the one in use stays shown as chosen. */
    fun of(role: String, s: SettingsView): List<Row> = SettingsView.providersFor(role).map { p ->
        val key = s.keys[p]
        val hint = when (key) {
            is KeyStatus.Found -> "Key found: ${key.file}"
            is KeyStatus.Problem -> key.detail
            null -> "Checking the key…"
        }
        val selected = s.effective(role).provider == p
        Row(p, Format.providerName(p), hint, selected, key?.usable == true && !selected)
    }

    /** "gpt-4o-mini · from the key file"; null when no model can run (summarise without a key). */
    fun model(role: String, s: SettingsView): String? {
        val e = s.effective(role)
        val m = e.model ?: return null
        val from = when (e.modelSource) {
            "override" -> "your choice"
            "keyFile" -> "from the key file"
            else -> "the default"
        }
        return "$m  ·  $from"
    }
}
