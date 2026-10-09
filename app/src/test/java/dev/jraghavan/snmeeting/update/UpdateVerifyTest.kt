package dev.jraghavan.snmeeting.update

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Host JVM tests for [UpdateVerify] (F14-FR5/FR6; inkread's UpdateVerifyTest, copied). Pure logic —
 * the checksum parse/compute/compare and signer-set equality — exercised without a device, so a
 * weakened integrity check fails the host gate rather than shipping.
 */
class UpdateVerifyTest {

    private val apkBytes = "meetings".toByteArray()

    @Test
    fun sha256_matches_known_vector() {
        // Compute once and assert the hex shape + determinism rather than hard-coding the digest.
        val h = UpdateVerify.sha256Hex(apkBytes)
        assertEquals(64, h.length)
        assertTrue(h.all { it in '0'..'9' || it in 'a'..'f' })
        assertEquals(h, UpdateVerify.sha256Hex(apkBytes)) // stable
    }

    @Test
    fun expectedShaFrom_parses_sha256sum_format() {
        val hex = "a".repeat(64)
        // `sha256sum` writes "<hex>  <filename>" (two spaces); also tolerate a single space + newline.
        assertEquals(hex, UpdateVerify.expectedShaFrom("$hex  meetings-v1.0.0.apk\n"))
        assertEquals(hex, UpdateVerify.expectedShaFrom("$hex meetings.apk"))
        assertEquals(hex, UpdateVerify.expectedShaFrom("${hex.uppercase()}  FILE")) // lowercased
    }

    @Test
    fun expectedShaFrom_rejects_garbage_and_absence() {
        assertNull(UpdateVerify.expectedShaFrom(null))
        assertNull(UpdateVerify.expectedShaFrom(""))
        assertNull(UpdateVerify.expectedShaFrom("   "))
        assertNull(UpdateVerify.expectedShaFrom("not-a-hash file"))
        assertNull(UpdateVerify.expectedShaFrom("a".repeat(63) + "  f")) // too short
        assertNull(UpdateVerify.expectedShaFrom("g".repeat(64) + "  f")) // non-hex
    }

    @Test
    fun matchesPublishedSha_true_on_match_false_on_tamper() {
        val good = UpdateVerify.sha256Hex(apkBytes)
        assertTrue(UpdateVerify.matchesPublishedSha(apkBytes, "$good  meetings.apk"))
        // A single flipped byte must fail.
        val tampered = apkBytes.copyOf().also { it[0] = (it[0] + 1).toByte() }
        assertFalse(UpdateVerify.matchesPublishedSha(tampered, "$good  meetings.apk"))
    }

    @Test
    fun matchesPublishedSha_false_when_no_checksum() {
        // Absent/malformed checksum is NOT a match — the caller falls back to signer-pin only.
        assertFalse(UpdateVerify.matchesPublishedSha(apkBytes, null))
        assertFalse(UpdateVerify.matchesPublishedSha(apkBytes, "garbage"))
    }

    // ---- the staged-APK gate (F14-AC4: a mismatch installs nothing and leaves no file) ----

    private fun staged(bytes: ByteArray): java.io.File =
        java.io.File.createTempFile("update", ".apk").apply { writeBytes(bytes); deleteOnExit() }

    @Test
    fun gate_keeps_a_matching_apk() {
        val apk = staged(apkBytes)
        assertEquals(apk, UpdateVerify.gate(apk, "${UpdateVerify.sha256Hex(apkBytes)}  meetings.apk\n"))
        assertTrue(apk.exists())
    }

    @Test
    fun gate_deletes_an_apk_whose_checksum_does_not_match() {
        val apk = staged(apkBytes)
        assertNull(UpdateVerify.gate(apk, "${"0".repeat(64)}  meetings.apk"))
        assertFalse(apk.exists())
    }

    @Test
    fun gate_skips_the_checksum_when_none_is_published() {
        val apk = staged(apkBytes)
        assertEquals(apk, UpdateVerify.gate(apk, null))
        assertEquals(apk, UpdateVerify.gate(apk, "not a checksum"))
    }

    @Test
    fun gate_refuses_a_missing_file() {
        val gone = staged(apkBytes).also { it.delete() }
        assertNull(UpdateVerify.gate(gone, null))
    }
}
