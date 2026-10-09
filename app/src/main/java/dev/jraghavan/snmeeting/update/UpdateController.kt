package dev.jraghavan.snmeeting.update

import android.content.Context
import android.util.Log
import dev.jraghavan.snmeeting.BuildConfig
import dev.jraghavan.snmeeting.NativeBridge
import org.json.JSONObject
import java.io.File

/**
 * The shell's half of the in-app updater (F14-FR4/FR5), from inkread's `UpdateController`
 * (ADR-INKREAD-0014). The network and the install live here; the Rust core only decides: this
 * fetches the GitHub `releases/latest` payload, hands it to [NativeBridge.nativeUpdateDecide]
 * for the semver decision, then on the user's say-so downloads the APK and verifies it
 * ([UpdateVerify]).
 *
 * Every method here blocks on I/O; callers invoke them off the UI thread ([UpdateGate]). The
 * controller is mechanism only: the skip/throttle/busy policy lives in [UpdateGate].
 */
class UpdateController(private val context: Context) {

    /** A newer release the core surfaced: the parsed [NativeBridge.nativeUpdateDecide] decision. */
    data class Available(
        val version: String,
        val notes: String,
        val apkUrl: String,
        val sha256Url: String,
    )

    /**
     * The outcome of a [check]: a newer release, nothing newer, nothing published, or the check
     * never reached the server. [Failed] leaves the throttle untouched so an offline launch can
     * retry. [NotPublished] is a Meetings addition: the API answers 404 for a private repo or one
     * with no release yet (ADR D13), which is quiet on launch and "Updates aren't available yet"
     * on a manual check (F14-AC3).
     */
    sealed interface CheckResult {
        data class Update(val available: Available) : CheckResult
        object UpToDate : CheckResult
        object NotPublished : CheckResult
        object Failed : CheckResult
    }

    /** The installed `versionName` (the basis for the semver comparison), or "" if unavailable. */
    fun installedVersion(): String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            .getOrNull()
            .orEmpty()

    /** Fetch `releases/latest` and ask the core whether it is newer. Blocking. */
    fun check(): CheckResult {
        val installed = installedVersion()
        if (installed.isEmpty()) return CheckResult.Failed
        val r = HttpFetch.getTextWithStatus(LATEST_RELEASE_URL, USER_AGENT, ACCEPT_GITHUB, TIMEOUT_MS, MAX_TEXT_BYTES)
        return resultOf(r.status, r.body) { NativeBridge.nativeUpdateDecide(installed, it) }
    }

    /**
     * Download [a]'s APK to the one private cache slot and verify it: the published SHA-256 (when
     * present) must match. Returns the verified file, or `null` on any failure, with the file
     * gone. Blocking.
     */
    fun downloadAndVerify(a: Available): File? {
        val dir = File(context.cacheDir, UPDATE_DIR).apply { mkdirs() }
        // One slot, overwritten each attempt: never accumulate stale APKs.
        val apk = File(dir, "update.apk")
        if (!HttpFetch.download(a.apkUrl, apk, USER_AGENT, TIMEOUT_MS, MAX_APK_BYTES)) {
            apk.delete()
            return null
        }
        val checksumText = if (a.sha256Url.isNotEmpty()) {
            HttpFetch.getText(a.sha256Url, USER_AGENT, null, TIMEOUT_MS, MAX_TEXT_BYTES)
        } else {
            null
        }
        return UpdateVerify.gate(apk, checksumText).also {
            if (it == null) Log.w(TAG, "update APK checksum mismatch; discarded")
        }
    }

    companion object {
        private const val TAG = "UpdateController"

        /** The releases endpoint; `/latest` already excludes drafts and prereleases. */
        val LATEST_RELEASE_URL = "https://api.github.com/repos/${BuildConfig.UPDATE_REPO}/releases/latest"
        private const val ACCEPT_GITHUB = "application/vnd.github+json"
        private const val USER_AGENT = "meetings-updater"

        /** Private cache subdir holding the single staged APK; shared with the install receiver. */
        const val UPDATE_DIR = "updates"
        private const val TIMEOUT_MS = 15_000
        private const val MAX_TEXT_BYTES = 512 * 1024 // release JSON / checksum line
        private const val MAX_APK_BYTES = 200L * 1024 * 1024 // APK ceiling (guards a hostile stream)

        /**
         * Map the HTTP answer and the core's decision to a [CheckResult], as inkread's `check`
         * does, plus 404 → [CheckResult.NotPublished] and a core error envelope → Failed.
         */
        fun resultOf(status: Int, body: String?, decide: (String) -> String): CheckResult {
            if (body == null) return if (status == 404) CheckResult.NotPublished else CheckResult.Failed
            val d = runCatching { JSONObject(decide(body)) }.getOrNull() ?: return CheckResult.Failed
            if (d.has("error")) return CheckResult.Failed
            if (!d.optBoolean("updateAvailable", false)) return CheckResult.UpToDate
            val apkUrl = d.optString("apkUrl")
            if (apkUrl.isEmpty()) return CheckResult.UpToDate
            return CheckResult.Update(
                Available(
                    version = d.optString("version"),
                    notes = d.optString("notes"),
                    apkUrl = apkUrl,
                    sha256Url = d.optString("sha256Url"),
                ),
            )
        }
    }
}
