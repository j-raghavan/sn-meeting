package dev.jraghavan.snmeeting.model

import org.json.JSONArray
import org.json.JSONObject
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * F16: what the shell shows of spend. The core prices and totals everything (spend.rs); these
 * only read its replies and lay them out.
 */
object Spend {
    /** A meeting's label from the core's `spend` object; "—" when there is none. */
    fun label(o: JSONObject?): String = o?.str("label") ?: "—"

    /** `YYYY-MM`, as nativeSpend takes it. */
    fun key(m: YearMonth): String = m.format(DateTimeFormatter.ofPattern("yyyy-MM", Locale.ROOT))

    /** The last [n] months up to [now], as nativeSpend's `(fromMonth, toMonth)`. */
    fun lastMonths(now: YearMonth, n: Int = 12): Pair<String, String> = key(now.minusMonths((n - 1).toLong())) to key(now)

    /** Cents, as the core rounds them: "$3.20", or "< $0.01" below a cent. */
    fun money(usd: Double): String =
        if (usd > 0 && usd < 0.005) "< \$0.01" else "\$" + String.format(Locale.ROOT, "%.2f", usd)

    /** "October 2026" for `2026-10`. */
    fun monthName(key: String): String = try {
        YearMonth.parse(key).format(DateTimeFormatter.ofPattern("MMMM yyyy", Locale.UK))
    } catch (e: Exception) {
        key
    }
}

/** One provider's spend in a month, by kind. */
data class ProviderSpend(val provider: String, val transcribe: Double, val summarise: Double)

/** One month of nativeSpend; [search] is the handwritten queries' cost (F19-FR9). */
data class SpendMonth(
    val month: String, val usd: Double, val meetings: Int, val unpriced: Int, val label: String,
    val byProvider: List<ProviderSpend>, val search: Double = 0.0, val searches: Int = 0,
) {
    companion object {
        fun from(o: JSONObject): SpendMonth? {
            val by = o.obj("byProvider")
            val providers = by?.keys()?.asSequence()?.sorted()?.mapNotNull { p ->
                val k = by.obj(p) ?: return@mapNotNull null
                ProviderSpend(p, k.num("transcribe"), k.num("summarise"))
            }?.toList() ?: emptyList()
            return SpendMonth(
                month = o.str("month") ?: return null,
                usd = o.num("usd"),
                meetings = o.int("meetings") ?: 0,
                unpriced = o.int("unpriced") ?: 0,
                label = o.strOr("label", ""),
                byProvider = providers,
                search = o.num("search"),
                searches = o.int("searches") ?: 0,
            )
        }
    }
}

/** nativeSpend's reply. */
data class SpendReport(val months: List<SpendMonth>, val unpriced: Int, val notes: List<String>) {
    /** Settings → SPEND's rows, newest month first: the month and its total, then a line per provider. */
    data class Row(val month: String, val total: String, val lines: List<String>)

    private fun spent(m: SpendMonth) = m.meetings > 0 || m.searches > 0

    /**
     * The months with spend, newest first and at most 12, and always the current (newest) one;
     * none at all when nothing has been spent ("Nothing spent yet").
     */
    fun rows(): List<Row> = if (months.none(::spent)) emptyList() else months.reversed()
        .filterIndexed { i, m -> i == 0 || spent(m) }.take(12).map { m ->
        val plus = if (m.unpriced > 0) "+" else ""
        val total = if (m.meetings == 0 && m.searches == 0) "—" else Spend.money(m.usd) + plus
        val lines = m.byProvider.map { p ->
            Format.providerName(p.provider) + "  ·  Transcription " + Spend.money(p.transcribe) +
                "  ·  Minutes " + Spend.money(p.summarise)
        } + (if (m.searches > 0) listOf("Search  ·  " + Spend.money(m.search)) else emptyList())
        Row(Spend.monthName(m.month), total, lines)
    }

    companion object {
        fun from(o: JSONObject): SpendReport = SpendReport(
            o.arr("months").mapObj(SpendMonth::from),
            o.int("unpriced") ?: 0,
            o.arr("notes").strings(),
        )
    }
}

/** One entry of nativeGetRates. [json] is the entry as the core sent it, sent back on save. */
data class RateEntry(
    val json: JSONObject, val provider: String, val model: String, val unit: String,
    val validFrom: String?, val asOf: String, val note: String?, val user: Boolean,
) {
    /** The editable prices for this entry's unit, in order, with their labels. */
    val fields: List<Pair<String, String>>
        get() = if (unit == "minute") listOf("perMinute" to "per minute")
        else listOf(
            "input" to "input / 1M", "cachedInput" to "cached input / 1M", "cacheWrite" to "cache write / 1M",
            "audioInput" to "audio input / 1M", "output" to "output / 1M", "perMinuteEstimate" to "per minute, estimate",
        )

    fun price(field: String): Double? = if (json.isNull(field) || !json.has(field)) null else (json.opt(field) as? Number)?.toDouble()

    /** "OpenAI · whisper-1", with "from 1 Jan 2027" for a scheduled price. */
    val title: String
        get() = Format.providerName(provider) + "  ·  " + model + (validFrom?.let { "  ·  from $it" } ?: "")

    companion object {
        fun from(o: JSONObject): RateEntry? { return RateEntry(
            json = o,
            provider = o.str("provider") ?: return null,
            model = o.str("model") ?: return null,
            unit = o.strOr("unit", "tokens"),
            validFrom = o.str("validFrom"),
            asOf = o.strOr("asOf", ""),
            note = o.str("note"),
            user = o.str("origin") == "user",
        ) }
    }
}

/** nativeGetRates: `{asOf, entries, problems}`. */
data class RatesView(val asOf: String, val entries: List<RateEntry>, val problems: List<String>) {
    companion object {
        fun from(o: JSONObject): RatesView = RatesView(
            o.strOr("asOf", ""), o.arr("entries").mapObj(RateEntry::from), o.arr("problems").strings(),
        )
    }
}

/** What the rates editor sends to nativeSetRates. */
object RatesEdit {
    /**
     * Every entry with the typed prices applied: [typed] maps `entryIndex to field` to the
     * field's text. An empty text clears the price; text that isn't a number is reported by
     * its entry's title instead of being sent.
     */
    fun build(entries: List<RateEntry>, typed: Map<Pair<Int, String>, String>): Pair<String?, String?> {
        val out = JSONArray()
        entries.forEachIndexed { i, e ->
            val o = JSONObject(e.json.toString())
            o.remove("origin")
            for ((field, _) in e.fields) {
                val t = typed[i to field]?.trim() ?: continue
                if (t.isEmpty()) { o.remove(field); continue }
                val v = t.toDoubleOrNull() ?: return null to "${e.title}: \"$t\" isn't a number"
                o.put(field, v)
            }
            out.put(o)
        }
        return out.toString() to null
    }

    /** "Reset to defaults": no user entries. */
    const val RESET = "[]"
}

/** nativeRepriceAll's dry run, as the confirmation states it (F16-FR6). */
data class Reprice(val meetings: Int, val oldUsd: Double, val newUsd: Double, val unpricedBefore: Int, val unpricedAfter: Int) {
    /** The question to confirm, or null when re-pricing would change nothing. */
    fun question(): String? {
        if (meetings == 0) return null
        val before = Spend.money(oldUsd) + if (unpricedBefore > 0) "+" else ""
        val after = Spend.money(newUsd) + if (unpricedAfter > 0) "+" else ""
        return "${Format.plural(meetings, "meeting")} will change. Their recorded total goes from $before to $after. " +
            "The new rates replace what was recorded."
    }

    companion object {
        fun from(o: JSONObject): Reprice = Reprice(
            o.int("meetings") ?: 0, o.num("oldUsd"), o.num("newUsd"),
            o.int("unpricedBefore") ?: 0, o.int("unpricedAfter") ?: 0,
        )
    }
}

internal fun JSONObject.num(k: String): Double = if (isNull(k)) 0.0 else (opt(k) as? Number)?.toDouble() ?: 0.0
