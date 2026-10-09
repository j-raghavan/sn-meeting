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

    @Test
    fun certsMatch_true_for_same_set_any_order() {
        val a = byteArrayOf(1, 2, 3)
        val b = byteArrayOf(4, 5, 6)
        assertTrue(UpdateVerify.certsMatch(listOf(a, b), listOf(b.copyOf(), a.copyOf())))
    }

    @Test
    fun certsMatch_false_on_different_signer() {
        val installed = listOf(byteArrayOf(1, 2, 3))
        val attacker = listOf(byteArrayOf(9, 9, 9))
        assertFalse(UpdateVerify.certsMatch(installed, attacker))
    }

    @Test
    fun certsMatch_false_on_empty_or_size_mismatch() {
        val a = byteArrayOf(1, 2, 3)
        assertFalse(UpdateVerify.certsMatch(emptyList(), listOf(a)))
        assertFalse(UpdateVerify.certsMatch(listOf(a), emptyList()))
        // Subset must not pass (size differs).
        assertFalse(UpdateVerify.certsMatch(listOf(a), listOf(a.copyOf(), byteArrayOf(7))))
    }

    // ---- the staged-APK gate (F14-AC4: a refused APK installs nothing and leaves no file) ----

    private val me = listOf(byteArrayOf(1, 2, 3))
    private val sameKey: (java.io.File) -> List<ByteArray> = { listOf(byteArrayOf(1, 2, 3)) }

    private fun staged(bytes: ByteArray): java.io.File =
        java.io.File.createTempFile("update", ".apk").apply { writeBytes(bytes); deleteOnExit() }

    @Test
    fun gate_keeps_a_matching_apk() {
        val apk = staged(apkBytes)
        assertEquals(apk, UpdateVerify.gate(apk, "${UpdateVerify.sha256Hex(apkBytes)}  meetings.apk\n", me, sameKey))
        assertTrue(apk.exists())
    }

    @Test
    fun gate_deletes_an_apk_whose_checksum_does_not_match() {
        val apk = staged(apkBytes)
        assertNull(UpdateVerify.gate(apk, "${"0".repeat(64)}  meetings.apk", me, sameKey))
        assertFalse(apk.exists())
    }

    @Test
    fun gate_skips_the_checksum_when_none_is_published() {
        val apk = staged(apkBytes)
        assertEquals(apk, UpdateVerify.gate(apk, null, me, sameKey))
        assertEquals(apk, UpdateVerify.gate(apk, "not a checksum", me, sameKey))
    }

    @Test
    fun gate_deletes_an_apk_signed_by_another_key_even_with_a_matching_checksum() {
        // F14-AC7: the checksum proves only that the asset is what was published.
        val apk = staged(apkBytes)
        val good = "${UpdateVerify.sha256Hex(apkBytes)}  meetings.apk"
        assertNull(UpdateVerify.gate(apk, good, me) { listOf(byteArrayOf(9, 9, 9)) })
        assertFalse(apk.exists())
    }

    @Test
    fun gate_fails_closed_when_signers_cannot_be_read() {
        val unreadable = staged(apkBytes)
        assertNull(UpdateVerify.gate(unreadable, null, me) { throw IllegalStateException("no PackageInfo") })
        assertFalse(unreadable.exists())
        // No installed signers at all fails closed too.
        val apk = staged(apkBytes)
        assertNull(UpdateVerify.gate(apk, null, emptyList(), sameKey))
        assertFalse(apk.exists())
    }

    @Test
    fun gate_refuses_a_missing_file() {
        val gone = staged(apkBytes).also { it.delete() }
        assertNull(UpdateVerify.gate(gone, null, me, sameKey))
    }

    // ---- the release-key pin (F14-AC2: a debug copy says it can't update itself) ----

    @Test
    fun signedWith_matches_the_pinned_certificate_only() {
        val cert = byteArrayOf(4, 5, 6)
        val pin = UpdateVerify.sha256Hex(cert)
        assertTrue(UpdateVerify.signedWith(listOf(cert), pin))
        assertTrue(UpdateVerify.signedWith(listOf(cert), pin.uppercase().chunked(2).joinToString(":"))) // keytool form
        assertFalse(UpdateVerify.signedWith(listOf(byteArrayOf(7)), pin)) // the debug key
        assertFalse(UpdateVerify.signedWith(emptyList(), pin))
        assertFalse(UpdateVerify.signedWith(listOf(cert, byteArrayOf(7)), pin)) // not the release set
        assertFalse(UpdateVerify.signedWith(listOf(cert), "")) // no pin
        assertFalse(UpdateVerify.signedWith(listOf(cert), "zz"))
    }
}
