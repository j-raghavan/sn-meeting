package dev.jraghavan.snmeeting.update

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

/** F14-FR4: the updater's one HTTP GET is HTTPS only and never reads past its cap. */
class HttpFetchTest {

    @Test
    fun only_https_is_fetched() {
        assertTrue(HttpFetch.isAllowed("https://api.github.com/repos/j-raghavan/sn-meeting/releases/latest"))
        assertTrue(HttpFetch.isAllowed("  HTTPS://github.com/x.apk"))
        assertFalse(HttpFetch.isAllowed("http://github.com/x.apk")) // cleartext, even to GitHub
        assertFalse(HttpFetch.isAllowed("http://192.168.1.20/x.apk")) // no LAN exception: nothing here is local
        assertFalse(HttpFetch.isAllowed("file:///sdcard/x.apk"))
        assertFalse(HttpFetch.isAllowed("ftp://example.test/x.apk"))
        assertFalse(HttpFetch.isAllowed(""))
        assertFalse(HttpFetch.isAllowed("https://"))
    }

    @Test
    fun readCapped_stops_at_the_cap() {
        val big = ByteArray(100_000) { 7 }
        val read = HttpFetch.readCapped(ByteArrayInputStream(big), 20_000)
        // The last chunk may cross the cap by less than one buffer (16 KiB), never more.
        assertTrue(read.size >= 20_000)
        assertTrue(read.size < 20_000 + 16 * 1024)
        assertArrayEquals(big, HttpFetch.readCapped(ByteArrayInputStream(big), 1 shl 20))
    }

    @Test
    fun copyCapped_refuses_a_stream_over_the_cap() {
        val out = ByteArrayOutputStream()
        assertFalse(HttpFetch.copyCapped(ByteArrayInputStream(ByteArray(10_001)), out, 10_000))
        val ok = ByteArrayOutputStream()
        assertTrue(HttpFetch.copyCapped(ByteArrayInputStream(ByteArray(10_000) { 1 }), ok, 10_000))
        assertEquals(10_000, ok.size())
    }
}
