package dev.jraghavan.snmeeting.update

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import dev.jraghavan.snmeeting.CoreJobs
import dev.jraghavan.snmeeting.InkDialog
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.SettingsActivity
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.SettingsPatch
import dev.jraghavan.snmeeting.model.SettingsView
import dev.jraghavan.snmeeting.model.UpdatePrefs
import dev.jraghavan.snmeeting.update.UpdateController.Available
import dev.jraghavan.snmeeting.update.UpdateController.CheckResult
import java.util.concurrent.Executors

/**
 * The updater's UX (F14-FR8), from inkread's `UpdateGate`: a throttled check on launch (never
 * background polling), "Check for updates" in Settings → About, and for a found update the
 * Install / Skip this version / Later choice, or under the opt-in auto-install toggle straight to
 * Android's installer. The decisions are [UpdatePolicy]'s.
 *
 * Meetings differences: nothing is checked, shown, downloaded or installed while [UpdateBusy];
 * a 404 is quiet on launch; and the outcome is kept in [state] so Settings shows it in place
 * (inkread toasts it). All blocking work runs on one daemon thread; every UI touch is on the main
 * thread and guarded against a finishing activity.
 */
object UpdateGate {

    sealed interface State {
        object Idle : State
        object Checking : State
        data class Line(val text: String) : State
        data class Found(val available: Available) : State
        data class Downloading(val version: String) : State
    }

    /** What Settings → About shows. Main thread. */
    var state: State = State.Idle
        private set
    private val listeners = LinkedHashSet<() -> Unit>()

    /** Main thread; the returned function unsubscribes. */
    fun observe(l: () -> Unit): () -> Unit {
        listeners += l
        return { listeners -= l }
    }

    private fun set(s: State) {
        state = s
        listeners.toList().forEach { it() }
    }

    private val main = Handler(Looper.getMainLooper())

    /** One reusable daemon thread for all update I/O (as inkread): sequential, never the UI's. */
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "meetings-update").apply { isDaemon = true } }

    /** From [dev.jraghavan.snmeeting.MainActivity.onResume]: honours the toggle, the throttle and the busy rule. */
    fun maybeCheckOnLaunch(activity: Activity) {
        if (UpdateBusy.now() || state == State.Checking || state is State.Downloading) return
        // settings.updates come from the core, queued behind nativeInit on CoreJobs.
        CoreJobs.run({ NativeBridge.nativeGetSettings() }) { json ->
            val prefs = CoreJson.obj(json, SettingsView::from).getOrNull()?.updates ?: return@run
            if (UpdatePolicy.launchCheckDue(prefs, System.currentTimeMillis(), UpdateBusy.now())) {
                runCheck(activity, manual = false, prefs)
            }
        }
    }

    /** "Check for updates" in Settings: ignores the throttle and always answers. */
    fun checkNow(activity: Activity, prefs: UpdatePrefs) {
        if (state == State.Checking || state is State.Downloading) return
        if (UpdateBusy.now()) return set(State.Line(UpdateBusy.MESSAGE))
        set(State.Checking)
        runCheck(activity, manual = true, prefs)
    }

    private fun runCheck(activity: Activity, manual: Boolean, prefs: UpdatePrefs) {
        val controller = UpdateController(activity.applicationContext)
        io.execute {
            // A copy signed with another key can never take a release (F14-AC2): don't fetch.
            if (!controller.canUpdateItself()) {
                main.post { if (manual) set(State.Line(UpdatePolicy.CANT_UPDATE)) }
                return@execute
            }
            val result = controller.check()
            if (UpdatePolicy.advancesThrottle(result)) {
                val now = System.currentTimeMillis()
                CoreJobs.exec { NativeBridge.nativeSetSettings(SettingsPatch.lastCheck(now)) }
            }
            val canInstall = controller.canRequestInstall()
            main.post { onResult(activity, controller, result, manual, prefs, canInstall) }
        }
    }

    private fun onResult(activity: Activity, controller: UpdateController, result: CheckResult, manual: Boolean, prefs: UpdatePrefs, canInstall: Boolean) {
        if (result !is CheckResult.Update) {
            val line = UpdatePolicy.line(result, manual)
            set(if (line != null) State.Line(line) else State.Idle)
            return
        }
        val a = result.available
        when (UpdatePolicy.route(manual, a, prefs, canInstall, UpdateBusy.now())) {
            UpdatePolicy.Route.SILENT -> if (manual) set(State.Found(a)) else set(State.Idle)
            UpdatePolicy.Route.AUTO_INSTALL -> downloadThenInstall(activity, controller, a)
            UpdatePolicy.Route.PROMPT -> {
                set(State.Found(a))
                if (!manual) prompt(activity, a) // Settings shows the choice in place
            }
        }
    }

    /** The launch prompt: Install / Skip this version / Later (as inkread). */
    private fun prompt(activity: Activity, a: Available) {
        InkDialog.choose(
            activity, "Update available: v${a.version}", "What's new:\n\n${UpdatePolicy.notes(a)}",
            "Install", "Skip this version", "Later",
            onYes = { install(activity, a) }, onNo = { skip(a) }, onLater = { later() },
        )
    }

    /** Install, from the prompt or Settings. Refused while recording or a job runs (F14-AC6). */
    fun install(activity: Activity, a: Available) {
        if (UpdateBusy.now()) return say(activity, UpdateBusy.MESSAGE)
        val controller = UpdateController(activity.applicationContext)
        if (!controller.canRequestInstall()) {
            say(activity, UpdatePolicy.NEEDS_GRANT)
            runCatching { activity.startActivity(controller.unknownSourcesSettingsIntent()) }
            return
        }
        downloadThenInstall(activity, controller, a)
    }

    /** Skip this version: launch checks stay quiet about it; a manual check still offers it. */
    fun skip(a: Available) {
        CoreJobs.exec { NativeBridge.nativeSetSettings(SettingsPatch.skipVersion(a.version)) }
        set(State.Line("v${a.version} skipped. Check for updates still offers it."))
    }

    fun later() = set(State.Idle)

    /** Download and verify off the UI thread, then hand the APK to Android's installer. */
    private fun downloadThenInstall(activity: Activity, controller: UpdateController, a: Available) {
        if (UpdateBusy.now()) return say(activity, UpdateBusy.MESSAGE)
        set(State.Downloading(a.version))
        io.execute {
            val apk = controller.downloadAndVerify(a)
            main.post {
                when {
                    apk == null -> say(activity, UpdatePolicy.DOWNLOAD_FAILED)
                    // A recording may have started during the download.
                    UpdateBusy.now() -> { apk.delete(); say(activity, UpdateBusy.MESSAGE) }
                    else -> runCatching { controller.install(apk) }
                        .onSuccess { set(State.Line("Android will ask you to confirm the update.")) }
                        .onFailure { apk.delete(); say(activity, UpdatePolicy.INSTALLER_FAILED) }
                }
            }
        }
    }

    /** Settings shows the line in place; elsewhere (Home's prompt) it is also a toast, as inkread. */
    private fun say(activity: Activity, text: String) {
        set(State.Line(text))
        if (activity !is SettingsActivity && !activity.isFinishing && !activity.isDestroyed) {
            Toast.makeText(activity, text, Toast.LENGTH_LONG).show()
        }
    }
}
