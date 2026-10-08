package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** S1: first-run permissions. */
class PermissionsTest {
    private val p = Permissions()

    @Test fun storageIsCheckedOnItsOwnAndNeverOpensSettingsByItself() {
        // Every resume with no all-files access shows the card; nothing is launched.
        repeat(3) { assertEquals(Permissions.OnResume.STORAGE_CARD, p.onResume(storage = false)) }
        assertEquals(Permissions.OnResume.LISTS, p.onResume(storage = true))
    }

    @Test fun theMicIsAskedForOnlyOnNew() {
        assertEquals(Permissions.OnResume.LISTS, p.onResume(storage = true)) // no mic prompt on launch
        assertEquals(Permissions.OnNew.ASK_MIC, p.onNew(mic = false, storage = true))
        p.micResult(granted = true)
        assertEquals(Permissions.OnNew.START, p.onNew(mic = true, storage = true))
    }

    @Test fun aMicDeniedTwiceStillReachesStorage() {
        p.micResult(granted = false); p.micResult(granted = false)
        assertEquals(Permissions.OnResume.STORAGE_CARD, p.onResume(storage = false))
        assertEquals(Permissions.OnNew.GRANT_STORAGE, p.onNew(mic = false, storage = false))
        // With storage granted, the mic can't be asked again: say where to allow it.
        assertEquals(Permissions.OnNew.MIC_BLOCKED, p.onNew(mic = false, storage = true))
        assertTrue(Permissions.MIC_BLOCKED_TEXT.contains("microphone"))
    }

    @Test fun storageDeniedShowsTheCardWithoutALoop() {
        assertEquals(Permissions.OnNew.GRANT_STORAGE, p.onNew(mic = true, storage = false))
        // Back from the system screen without granting: the card again, not the screen again.
        assertEquals(Permissions.OnResume.STORAGE_CARD, p.onResume(storage = false))
        assertEquals(Permissions.OnResume.STORAGE_CARD, p.onResume(storage = false))
        assertFalse(Permissions.STORAGE_TEXT.isEmpty())
    }
}
