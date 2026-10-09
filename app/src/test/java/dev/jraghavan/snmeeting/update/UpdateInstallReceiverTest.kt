package dev.jraghavan.snmeeting.update

import android.content.pm.PackageInstaller
import dev.jraghavan.snmeeting.update.UpdateInstallReceiver.Action
import org.junit.Assert.assertEquals
import org.junit.Test

/** F14-FR7 / F14-AC6: the OS confirmation is shown only when nothing is recording or running. */
class UpdateInstallReceiverTest {
    @Test
    fun pending_user_action_confirms_unless_busy() {
        assertEquals(Action.CONFIRM, UpdateInstallReceiver.actionFor(PackageInstaller.STATUS_PENDING_USER_ACTION, busy = false))
        assertEquals(Action.ABANDON, UpdateInstallReceiver.actionFor(PackageInstaller.STATUS_PENDING_USER_ACTION, busy = true))
    }

    @Test
    fun terminal_statuses_clear_the_cache_whatever_is_running() {
        for (busy in listOf(false, true)) {
            assertEquals(Action.INSTALLED, UpdateInstallReceiver.actionFor(PackageInstaller.STATUS_SUCCESS, busy))
            assertEquals(Action.FAILED, UpdateInstallReceiver.actionFor(PackageInstaller.STATUS_FAILURE_ABORTED, busy))
            assertEquals(Action.FAILED, UpdateInstallReceiver.actionFor(PackageInstaller.STATUS_FAILURE_INCOMPATIBLE, busy))
            assertEquals(Action.FAILED, UpdateInstallReceiver.actionFor(Int.MIN_VALUE, busy)) // no status
        }
    }
}
