package dev.jraghavan.snmeeting.update

import dev.jraghavan.snmeeting.model.UpdatePrefs
import dev.jraghavan.snmeeting.update.UpdateController.Available
import dev.jraghavan.snmeeting.update.UpdateController.CheckResult
import dev.jraghavan.snmeeting.update.UpdatePolicy.Route
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** F14-FR8 / F14-AC3 / F14-AC6: when the updater checks, what it does with an answer, what it says. */
class UpdatePolicyTest {
    private val hour = 60L * 60 * 1000
    private val now = 1_791_000_000_000L
    private val a = Available("0.2.0", "fixes", "https://x/a.apk", "")

    @Test
    fun the_launch_check_is_throttled_to_six_hours() {
        val p = UpdatePrefs(lastCheckMs = now - 5 * hour)
        assertFalse(UpdatePolicy.launchCheckDue(p, now, busy = false))
        assertTrue(UpdatePolicy.launchCheckDue(p.copy(lastCheckMs = now - 6 * hour), now, busy = false))
        assertTrue(UpdatePolicy.launchCheckDue(UpdatePrefs(), now, busy = false)) // never checked
        // A clock set back must not silence the check until it catches up.
        assertTrue(UpdatePolicy.launchCheckDue(p.copy(lastCheckMs = now + 48 * hour), now, busy = false))
    }

    @Test
    fun no_launch_check_when_turned_off_or_busy() {
        assertFalse(UpdatePolicy.launchCheckDue(UpdatePrefs(autoCheck = false), now, busy = false))
        assertFalse(UpdatePolicy.launchCheckDue(UpdatePrefs(), now, busy = true)) // recording or a job
    }

    @Test
    fun only_an_answer_from_the_server_advances_the_throttle() {
        assertFalse(UpdatePolicy.advancesThrottle(CheckResult.Failed))
        assertTrue(UpdatePolicy.advancesThrottle(CheckResult.UpToDate))
        assertTrue(UpdatePolicy.advancesThrottle(CheckResult.NotPublished)) // a 404 is an answer
        assertTrue(UpdatePolicy.advancesThrottle(CheckResult.Update(a)))
    }

    @Test
    fun a_found_update_prompts_unless_skipped_auto_installed_or_busy() {
        val p = UpdatePrefs()
        assertEquals(Route.PROMPT, UpdatePolicy.route(manual = false, a, p, canInstall = true, busy = false))
        val skipped = p.copy(skipVersion = "0.2.0")
        assertEquals(Route.SILENT, UpdatePolicy.route(manual = false, a, skipped, canInstall = true, busy = false))
        assertEquals("a manual check always shows it", Route.PROMPT, UpdatePolicy.route(manual = true, a, skipped, canInstall = true, busy = false))
        val auto = p.copy(autoInstall = true)
        assertEquals(Route.AUTO_INSTALL, UpdatePolicy.route(manual = false, a, auto, canInstall = true, busy = false))
        assertEquals("no grant yet: ask", Route.PROMPT, UpdatePolicy.route(manual = false, a, auto, canInstall = false, busy = false))
        assertEquals("auto-install needs auto-check", Route.PROMPT, UpdatePolicy.route(manual = false, a, auto.copy(autoCheck = false), canInstall = true, busy = false))
        assertEquals(Route.PROMPT, UpdatePolicy.route(manual = true, a, auto, canInstall = true, busy = false))
        // F14-AC6: nothing pops up over a recording.
        assertEquals(Route.SILENT, UpdatePolicy.route(manual = false, a, p, canInstall = true, busy = true))
        assertEquals(Route.SILENT, UpdatePolicy.route(manual = false, a, auto, canInstall = true, busy = true))
    }

    @Test
    fun a_launch_check_says_nothing_unless_there_is_an_update() {
        assertNull(UpdatePolicy.line(CheckResult.Failed, manual = false))
        assertNull(UpdatePolicy.line(CheckResult.UpToDate, manual = false))
        assertNull(UpdatePolicy.line(CheckResult.NotPublished, manual = false)) // F14-AC3: no error spam
    }

    @Test
    fun a_manual_check_always_answers() {
        assertEquals("Updates aren't available yet.", UpdatePolicy.line(CheckResult.NotPublished, manual = true))
        assertEquals("Meetings is up to date.", UpdatePolicy.line(CheckResult.UpToDate, manual = true))
        assertEquals("Couldn't check for updates. Check the Wi-Fi and try again.", UpdatePolicy.line(CheckResult.Failed, manual = true))
        assertEquals("Meetings v0.2.0 is available.", UpdatePolicy.line(CheckResult.Update(a), manual = true))
    }

    @Test
    fun release_notes_are_trimmed_for_the_prompt() {
        assertEquals("A newer version of Meetings is available.", UpdatePolicy.notes(a.copy(notes = "  ")))
        assertEquals("fixes", UpdatePolicy.notes(a))
        val long = UpdatePolicy.notes(a.copy(notes = "x".repeat(2000)))
        assertTrue(long.length <= 601 && long.endsWith("…"))
    }
}
