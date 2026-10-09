package dev.jraghavan.snmeeting.update

import dev.jraghavan.snmeeting.update.UpdateController.CheckResult
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** F14-FR5 / F14-AC3: how an HTTP answer and the core's decision become a check result. */
class UpdateControllerTest {

    private fun never(): (String) -> String = { error("the core must not be asked") }

    @Test
    fun a_404_is_not_published_and_never_reaches_the_core() {
        assertSame(CheckResult.NotPublished, UpdateController.resultOf(404, null, never()))
    }

    @Test
    fun no_answer_or_another_error_status_is_failed() {
        assertSame(CheckResult.Failed, UpdateController.resultOf(0, null, never()))
        assertSame(CheckResult.Failed, UpdateController.resultOf(403, null, never())) // rate limited
        assertSame(CheckResult.Failed, UpdateController.resultOf(500, null, never()))
    }

    @Test
    fun the_core_saying_no_is_up_to_date() {
        assertSame(CheckResult.UpToDate, UpdateController.resultOf(200, "{}") { """{"updateAvailable":false}""" })
        // An offer without an APK URL can't be acted on.
        assertSame(CheckResult.UpToDate, UpdateController.resultOf(200, "{}") { """{"updateAvailable":true,"version":"9.0.0"}""" })
    }

    @Test
    fun a_core_error_or_junk_is_failed() {
        assertSame(CheckResult.Failed, UpdateController.resultOf(200, "{}") { """{"error":"internal error"}""" })
        assertSame(CheckResult.Failed, UpdateController.resultOf(200, "{}") { "not json" })
        assertSame(CheckResult.Failed, UpdateController.resultOf(200, "{}") { throw UnsatisfiedLinkError("no core") })
    }

    @Test
    fun an_offer_carries_the_assets() {
        var seen = ""
        val r = UpdateController.resultOf(200, "BODY") {
            seen = it
            """{"updateAvailable":true,"version":"0.2.0","notes":"fixes","apkUrl":"https://x/a.apk","sha256Url":"https://x/a.apk.sha256"}"""
        }
        assertEquals("BODY", seen)
        assertTrue(r is CheckResult.Update)
        assertEquals(
            UpdateController.Available("0.2.0", "fixes", "https://x/a.apk", "https://x/a.apk.sha256"),
            (r as CheckResult.Update).available,
        )
    }

    @Test
    fun the_releases_url_is_this_repo_over_https() {
        assertEquals("https://api.github.com/repos/j-raghavan/sn-meeting/releases/latest", UpdateController.LATEST_RELEASE_URL)
        assertTrue(HttpFetch.isAllowed(UpdateController.LATEST_RELEASE_URL))
    }
}
