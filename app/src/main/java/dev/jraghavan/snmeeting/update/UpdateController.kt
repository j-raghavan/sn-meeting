package dev.jraghavan.snmeeting.update

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
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
 * ([UpdateVerify]: published SHA-256 + installed-signer pin).
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
     * Whether this copy is signed with the release key (BuildConfig.RELEASE_CERT_SHA256). A copy
     * signed with another key, such as a local debug build, can't take a release update, so
     * [UpdateGate] says so rather than offer one (F14-AC2).
     */
    fun canUpdateItself(): Boolean = UpdateVerify.signedWith(installedSignerCerts(), BuildConfig.RELEASE_CERT_SHA256)

    /**
     * Download [a]'s APK to the one private cache slot and verify it: the published SHA-256 (when
     * present) must match, and the signer set must equal the installed app's (F14-FR6, fails
     * closed). Returns the verified file, or `null` on any failure, with the file gone. Blocking.
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
        return UpdateVerify.gate(apk, checksumText, installedSignerCerts(), ::archiveSignerCerts).also {
            if (it == null) Log.w(TAG, "update APK failed its checksum or signer check; discarded")
        }
    }

    /** Whether the OS will let this app launch an install (the "install unknown apps" grant). */
    fun canRequestInstall(): Boolean = context.packageManager.canRequestPackageInstalls()

    /** The per-app "install unknown apps" screen, so the user can grant it once (F14-FR7). */
    fun unknownSourcesSettingsIntent(): Intent =
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))

    /**
     * Hand [apk] to the system installer through a [PackageInstaller] session (streams the bytes;
     * no FileProvider). The OS shows its own final confirmation; the outcome arrives at
     * [UpdateInstallReceiver]. Throws only when the session can't be opened; the caller reports it.
     */
    fun install(apk: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(context.packageName)
        val sessionId = installer.createSession(params)
        installer.openSession(sessionId).use { session ->
            session.openWrite("meetings", 0, apk.length()).use { out ->
                apk.inputStream().use { it.copyTo(out) }
                session.fsync(out)
            }
            val intent = Intent(context, UpdateInstallReceiver::class.java).setAction(ACTION_INSTALLED)
            // The installer fills in the status extras, so the PendingIntent must be mutable; it
            // names our receiver explicitly.
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
            val pending = PendingIntent.getBroadcast(context, sessionId, intent, flags)
            session.commit(pending.intentSender)
        }
    }

    // ---- signer extraction (minSdk 30: GET_SIGNING_CERTIFICATES only) ----

    private fun installedSignerCerts(): List<ByteArray> = runCatching {
        certBytesOf(context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES))
    }.getOrDefault(emptyList())

    private fun archiveSignerCerts(apk: File): List<ByteArray> =
        context.packageManager.getPackageArchiveInfo(apk.absolutePath, PackageManager.GET_SIGNING_CERTIFICATES)
            ?.let { certBytesOf(it) }
            .orEmpty()

    // apkContentsSigners = the certs the package is CURRENTLY signed by, symmetric for an installed
    // app and a downloaded archive. signingCertificateHistory would add the on-device rotation
    // lineage the archive lacks, which certsMatch's size check would then falsely reject (inkread).
    private fun certBytesOf(info: PackageInfo): List<ByteArray> =
        info.signingInfo?.apkContentsSigners?.map { it.toByteArray() }.orEmpty()

    companion object {
        private const val TAG = "UpdateController"

        /** The releases endpoint; `/latest` already excludes drafts and prereleases. */
        val LATEST_RELEASE_URL = "https://api.github.com/repos/${BuildConfig.UPDATE_REPO}/releases/latest"
        private const val ACCEPT_GITHUB = "application/vnd.github+json"
        private const val USER_AGENT = "meetings-updater"
        private const val ACTION_INSTALLED = "dev.jraghavan.snmeeting.UPDATE_INSTALLED"

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
