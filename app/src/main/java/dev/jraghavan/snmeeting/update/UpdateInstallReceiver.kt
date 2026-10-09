package dev.jraghavan.snmeeting.update

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.util.Log
import java.io.File

/**
 * Receives the [PackageInstaller] session callback for an update (F14-FR7), from inkread's
 * `UpdateInstallReceiver`. When the session reaches [PackageInstaller.STATUS_PENDING_USER_ACTION]
 * the OS hands back its confirmation intent, which we launch: Android's own "update this app?"
 * dialog (a sideloaded app can't install silently). Terminal statuses log and clear the cache.
 *
 * Meetings addition: the busy rule is checked again here, because the session may finish
 * writing after a recording or a transcription started ([UpdateBusy]). Then the session is
 * abandoned rather than confirmed; the user can install again once it is done.
 */
class UpdateInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, Int.MIN_VALUE)
        when (actionFor(status, UpdateBusy.now())) {
            Action.CONFIRM -> {
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                if (confirm != null) {
                    confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    context.startActivity(confirm)
                } else {
                    Log.w(TAG, "pending user action with no confirmation intent")
                }
            }
            Action.ABANDON -> {
                Log.w(TAG, "a recording or job started; update install abandoned")
                val id = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1)
                if (id >= 0) runCatching { context.packageManager.packageInstaller.abandonSession(id) }
                clearCache(context)
            }
            Action.INSTALLED -> {
                Log.i(TAG, "update installed")
                clearCache(context)
            }
            Action.FAILED -> {
                Log.w(TAG, "update install failed (status=$status): ${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)}")
                clearCache(context)
            }
        }
    }

    /** Drop the staged APK once the session is over: never leave it in cache. */
    private fun clearCache(context: Context) {
        runCatching { File(context.cacheDir, UpdateController.UPDATE_DIR).deleteRecursively() }
    }

    enum class Action { CONFIRM, ABANDON, INSTALLED, FAILED }

    companion object {
        private const val TAG = "UpdateInstall"

        /** What a session status means, given whether a recording or job is running now. */
        fun actionFor(status: Int, busy: Boolean): Action = when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> if (busy) Action.ABANDON else Action.CONFIRM
            PackageInstaller.STATUS_SUCCESS -> Action.INSTALLED
            else -> Action.FAILED
        }
    }
}
