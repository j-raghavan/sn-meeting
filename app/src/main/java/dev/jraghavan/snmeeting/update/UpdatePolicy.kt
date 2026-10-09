package dev.jraghavan.snmeeting.update

import dev.jraghavan.snmeeting.model.UpdatePrefs
import dev.jraghavan.snmeeting.update.UpdateController.Available
import dev.jraghavan.snmeeting.update.UpdateController.CheckResult

/**
 * The updater's policy (F14-FR8), the decisions inkread's `UpdateGate` makes inline, kept free of
 * Android so they are host-tested: when a launch check is due, what a found update leads to, and
 * the line a check leaves on the screen. Meetings adds the busy rule ([UpdateBusy]) and the 404
 * line (F14-AC3).
 */
object UpdatePolicy {
    /** At most one launch check per window: a sideloaded device is often offline, and Home
     *  never waits on the network (as inkread). */
    const val THROTTLE_MS = 6L * 60 * 60 * 1000

    private const val NOTES_LIMIT = 600

    /** On launch: auto-check is on, nothing is recording or running, and the window has passed
     *  (or the clock went back past the last check). */
    fun launchCheckDue(p: UpdatePrefs, nowMs: Long, busy: Boolean): Boolean {
        if (!p.autoCheck || busy) return false
        val since = nowMs - p.lastCheckMs
        return since < 0 || since >= THROTTLE_MS
    }

    /** Only a check that reached the server advances the throttle, so an offline launch can
     *  retry as soon as the network is back (as inkread). A 404 is an answer. */
    fun advancesThrottle(r: CheckResult): Boolean = r !is CheckResult.Failed

    enum class Route { SILENT, AUTO_INSTALL, PROMPT }

    /** A found update: a skipped version stays quiet on an automatic check (a manual one always
     *  shows it); opt-in auto-install goes straight to download and install; otherwise ask.
     *  Nothing is shown over a recording or a running job. */
    fun route(manual: Boolean, a: Available, p: UpdatePrefs, canInstall: Boolean, busy: Boolean): Route = when {
        !manual && busy -> Route.SILENT
        !manual && a.version == p.skipVersion -> Route.SILENT
        !manual && p.autoInstallEffective && canInstall -> Route.AUTO_INSTALL
        else -> Route.PROMPT
    }

    /** The line a check leaves in Settings → About; `null` = say nothing (a launch check that
     *  found nothing, could not reach GitHub, or found no releases). */
    fun line(r: CheckResult, manual: Boolean): String? = when (r) {
        is CheckResult.Update -> "Meetings v${r.available.version} is available."
        else -> if (!manual) null else when (r) {
            CheckResult.UpToDate -> "Meetings is up to date."
            CheckResult.NotPublished -> "Updates aren't available yet."
            else -> "Couldn't check for updates. Check the Wi-Fi and try again."
        }
    }

    /** The release notes, trimmed: enough to decide, not a wall of text (as inkread). */
    fun notes(a: Available): String {
        val n = a.notes.trim()
        if (n.isEmpty()) return "A newer version of Meetings is available."
        return if (n.length > NOTES_LIMIT) n.take(NOTES_LIMIT).trimEnd() + "…" else n
    }

    const val CANT_UPDATE = "This copy can't update itself: it isn't signed with the Meetings release key. Install a release from GitHub once to get updates."
    const val NEEDS_GRANT = "Allow Meetings to install apps, then tap Install again."
    const val DOWNLOAD_FAILED = "The update didn't download or didn't verify, so nothing was installed."
    const val INSTALLER_FAILED = "Couldn't start the installer."
}
