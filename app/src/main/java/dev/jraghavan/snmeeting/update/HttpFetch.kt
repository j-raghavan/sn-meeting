package dev.jraghavan.snmeeting.update

import android.util.Log
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URL

/**
 * The updater's one bounded HTTP GET (F14-FR4), from inkread's `HttpFetch`: Kotlin does the
 * update I/O and the core only decides (ADR D11). Both entry points cap the response so a
 * runaway or hostile body can't exhaust memory or disk, and both swallow failures into a
 * `null`/`false` so callers degrade quietly (offline means no update).
 *
 * HTTPS only. inkread allows cleartext to its LAN catalog servers; Meetings has no such caller
 * and its manifest disables cleartext, so anything but `https://` is refused here as well. No
 * Authorization header: nothing the updater fetches needs one, and a token never ships in the
 * APK (ADR D13).
 */
object HttpFetch {

    /** Whether [url] may be fetched: an `https://` URL with a host. */
    fun isAllowed(url: String): Boolean {
        val t = url.trim()
        if (!t.lowercase().startsWith("https://")) return false
        val host = t.substring("https://".length).substringBefore('/').substringBefore('?').substringBefore('#')
        return host.isNotBlank()
    }

    /** GET [url] as UTF-8 text, capped at [capBytes]; `null` on a non-2xx status or an I/O error. */
    fun getText(url: String, userAgent: String, accept: String?, timeoutMs: Int, capBytes: Int): String? =
        getTextWithStatus(url, userAgent, accept, timeoutMs, capBytes).body

    /**
     * The same GET, with the HTTP [Response.status] alongside the body: the updater tells "not
     * published" (404, F14-AC3) from "couldn't reach the server". `status` is 0 when the request
     * never got a response.
     */
    fun getTextWithStatus(url: String, userAgent: String, accept: String?, timeoutMs: Int, capBytes: Int): Response {
        if (url.isBlank()) return Response(0, null)
        return try {
            val conn = open(url, userAgent, accept, timeoutMs)
            val code = conn.responseCode
            if (code !in 200..299) {
                Log.w(TAG, "GET -> HTTP $code")
                Response(code, null)
            } else {
                Response(code, conn.inputStream.use { String(readCapped(it, capBytes), Charsets.UTF_8) })
            }
        } catch (e: Exception) {
            Log.w(TAG, "GET failed: ${e.javaClass.simpleName}")
            Response(0, null)
        }
    }

    /** An HTTP response: the [status] (0 = never reached the server) and the body when 2xx. */
    data class Response(val status: Int, val body: String?)

    /** Stream [url] to [dest], aborting past [capBytes]; `false` on non-2xx, an I/O error or oversize. */
    fun download(url: String, dest: File, userAgent: String, timeoutMs: Int, capBytes: Long): Boolean {
        if (url.isBlank()) return false
        return try {
            val conn = open(url, userAgent, null, timeoutMs)
            if (conn.responseCode !in 200..299) {
                Log.w(TAG, "download -> HTTP ${conn.responseCode}")
                return false
            }
            val ok = conn.inputStream.use { input -> dest.outputStream().use { copyCapped(input, it, capBytes) } }
            if (!ok) Log.w(TAG, "download exceeds the ${capBytes}-byte cap")
            ok
        } catch (e: Exception) {
            Log.w(TAG, "download failed: ${e.javaClass.simpleName}")
            false
        }
    }

    private fun open(url: String, userAgent: String, accept: String?, timeoutMs: Int): HttpURLConnection {
        // Both entry points funnel through here, so this is the one place the HTTPS rule has to
        // hold. Thrown: every caller already treats an exception as "the request did not happen".
        if (!isAllowed(url)) throw java.io.IOException("only https:// is fetched")
        return (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = timeoutMs
            readTimeout = timeoutMs
            // GitHub asset URLs redirect once to an HTTPS CDN; HttpURLConnection follows only a
            // same-protocol redirect, so HTTPS can't be downgraded on the way.
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", userAgent)
            if (accept != null) setRequestProperty("Accept", accept)
        }
    }

    /** Read up to [cap] bytes (the last chunk may cross it by under 16 KiB), then stop. */
    internal fun readCapped(input: InputStream, cap: Int): ByteArray {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(16 * 1024)
        var total = 0
        while (total < cap) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            total += n
        }
        return out.toByteArray()
    }

    /** Copy [input] to [out]; `false` as soon as more than [cap] bytes arrive. */
    internal fun copyCapped(input: InputStream, out: OutputStream, cap: Long): Boolean {
        val buf = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) return true
            total += n
            if (total > cap) return false
            out.write(buf, 0, n)
        }
    }

    private const val TAG = "HttpFetch"
}
