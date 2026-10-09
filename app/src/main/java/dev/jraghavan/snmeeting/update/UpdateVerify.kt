package dev.jraghavan.snmeeting.update

import java.io.File
import java.security.MessageDigest

/**
 * Pure integrity checks for a downloaded update APK (F14-FR5/FR6), copied from inkread's
 * `UpdateVerify` (ADR-INKREAD-0014 UPD-FR3/FR4). Free of Android types so it is host-tested;
 * [UpdateController] does the Android I/O and delegates the comparisons here.
 *
 * Two independent gates must both pass before the system installer is invoked:
 *  - **checksum** ([matchesPublishedSha]): the bytes are exactly what the release published;
 *  - **signer pin** ([certsMatch], F14-FR6): the APK is signed by *this* installed app's key.
 */
object UpdateVerify {

    /** Length of a SHA-256 digest as lowercase hex. */
    private const val SHA256_HEX_LEN = 64

    /**
     * Extract the expected hex digest from a `sha256sum`-style checksum file, whose first line is
     * `"<64-hex>  <filename>"`. Returns the lowercased digest, or `null` if the text carries no
     * well-formed 64-char hex token — the caller treats `null` as "no checksum published" (fall
     * back to signer-pin only) and a *present but mismatched* digest as a hard abort, so the two
     * cases must stay distinguishable.
     */
    fun expectedShaFrom(checksumFileText: String?): String? {
        if (checksumFileText.isNullOrBlank()) return null
        val token = checksumFileText.trimStart().substringBefore(' ').substringBefore('\n').trim()
        val hex = token.lowercase()
        return if (isSha256Hex(hex)) hex else null
    }

    /** Lowercase hex SHA-256 of [bytes]. */
    fun sha256Hex(bytes: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
        val sb = StringBuilder(digest.size * 2)
        for (b in digest) {
            val v = b.toInt() and 0xFF
            sb.append(HEX[v ushr 4]).append(HEX[v and 0x0F])
        }
        return sb.toString()
    }

    /**
     * Whether [apkBytes] hashes to the digest published in [checksumFileText]. Returns `false` when
     * the checksum text is missing/malformed — so the caller MUST first decide via [expectedShaFrom]
     * whether a checksum was published at all (absent ⇒ skip this gate; present ⇒ require a match).
     */
    fun matchesPublishedSha(apkBytes: ByteArray, checksumFileText: String?): Boolean {
        val expected = expectedShaFrom(checksumFileText) ?: return false
        return constantTimeEquals(expected, sha256Hex(apkBytes))
    }

    /**
     * Whether the candidate APK's signer set equals the installed app's signer set. Both lists hold
     * the raw signing-certificate bytes (`Signature.toByteArray()`); a match requires the same
     * multiset of certificates and at least one signer on each side (an empty set never matches —
     * an unsigned or signer-stripped APK is rejected).
     */
    fun certsMatch(installed: List<ByteArray>, candidate: List<ByteArray>): Boolean {
        if (installed.isEmpty() || candidate.isEmpty()) return false
        if (installed.size != candidate.size) return false
        val remaining = candidate.toMutableList()
        for (cert in installed) {
            val idx = remaining.indexOfFirst { it.contentEquals(cert) }
            if (idx < 0) return false
            remaining.removeAt(idx)
        }
        return remaining.isEmpty()
    }

    /**
     * The staged-APK gate [UpdateController.downloadAndVerify] applies (F14-AC4): a *published*
     * digest must match the file (an absent one skips this gate, as inkread), and the file's
     * signers ([candidate], read from it) must equal the [installed] app's ([certsMatch]).
     * Returns [apk] when both pass; otherwise deletes it (nothing is left in the cache slot) and
     * returns `null`.
     */
    fun gate(apk: File, checksumText: String?, installed: List<ByteArray>, candidate: (File) -> List<ByteArray>): File? {
        val bytes = runCatching { apk.readBytes() }.getOrNull()
        val ok = bytes != null &&
            (expectedShaFrom(checksumText) == null || matchesPublishedSha(bytes, checksumText)) &&
            certsMatch(installed, runCatching { candidate(apk) }.getOrDefault(emptyList()))
        if (!ok) apk.delete()
        return if (ok) apk else null
    }

    /**
     * Whether the installed app is signed by the release key whose certificate SHA-256 is [pinHex]
     * (BuildConfig.RELEASE_CERT_SHA256). A copy signed with another key (a local debug build) can
     * never take a release update, so Settings says so instead of offering one (F14-AC2). This
     * only decides what to show: installs are gated on [certsMatch].
     */
    fun signedWith(installed: List<ByteArray>, pinHex: String): Boolean {
        val pin = pinHex.replace(":", "").lowercase()
        return isSha256Hex(pin) && installed.size == 1 && constantTimeEquals(pin, sha256Hex(installed[0]))
    }

    private fun isSha256Hex(s: String): Boolean =
        s.length == SHA256_HEX_LEN && s.all { it in '0'..'9' || it in 'a'..'f' }

    /** Length-aware constant-time hex compare (inputs are already lowercased). */
    private fun constantTimeEquals(a: String, b: String): Boolean {
        if (a.length != b.length) return false
        var diff = 0
        for (i in a.indices) diff = diff or (a[i].code xor b[i].code)
        return diff == 0
    }

    private val HEX = "0123456789abcdef".toCharArray()
}
