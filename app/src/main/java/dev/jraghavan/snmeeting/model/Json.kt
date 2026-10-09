package dev.jraghavan.snmeeting.model

import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

/**
 * Null-safe reads that behave the same on Android's org.json and on json.org's (used by the JVM
 * tests). Android's optString(k) returns "null" for a JSON null; json.org's returns "". So no
 * parser calls optString/optLong directly: everything goes through these.
 */
internal fun JSONObject.str(k: String): String? = if (isNull(k)) null else opt(k) as? String
internal fun JSONObject.strOr(k: String, d: String): String = str(k) ?: d
internal fun JSONObject.long(k: String): Long? = if (isNull(k)) null else (opt(k) as? Number)?.toLong()
internal fun JSONObject.int(k: String): Int? = long(k)?.toInt()
internal fun JSONObject.bool(k: String): Boolean? = if (isNull(k)) null else opt(k) as? Boolean
internal fun JSONObject.obj(k: String): JSONObject? = opt(k) as? JSONObject
internal fun JSONObject.arr(k: String): JSONArray? = opt(k) as? JSONArray

internal fun <T> JSONArray?.mapObj(f: (JSONObject) -> T?): List<T> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { i -> (opt(i) as? JSONObject)?.let(f) }
}

internal fun JSONArray?.strings(): List<String> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { i -> opt(i) as? String }
}

/** What every core call returns: a value, or the core's `{error, field?, line?}` envelope. */
sealed class CoreResult<out T> {
    data class Ok<T>(val value: T) : CoreResult<T>()
    data class Err(val message: String, val field: String? = null, val line: Int? = null) : CoreResult<Nothing>()

    fun getOrNull(): T? = (this as? Ok<T>)?.value
}

object CoreJson {
    /** Parses [json]; an `{error: "..."}` object, malformed JSON or a shape [f] rejects becomes [CoreResult.Err]. */
    fun <T> parse(json: String, f: (Any) -> T?): CoreResult<T> {
        val v = try {
            JSONTokener(json).nextValue()
        } catch (e: Exception) {
            return CoreResult.Err("unreadable reply from the core")
        }
        if (v is JSONObject && v.str("error") != null) {
            return CoreResult.Err(v.strOr("error", "error"), v.str("field"), v.int("line"))
        }
        val out = try { f(v) } catch (e: Exception) { null }
        return if (out != null) CoreResult.Ok(out) else CoreResult.Err("unexpected reply from the core")
    }

    fun <T> obj(json: String, f: (JSONObject) -> T?): CoreResult<T> = parse(json) { (it as? JSONObject)?.let(f) }
    fun <T> list(json: String, f: (JSONObject) -> T?): CoreResult<List<T>> = parse(json) { (it as? JSONArray)?.mapObj(f) }
    fun ok(json: String): CoreResult<Unit> = obj(json) { }

    /** A JSON string literal (for patches the shell builds); JSONObject.quote exists in both libraries. */
    fun quote(s: String): String = JSONObject.quote(s)
}
