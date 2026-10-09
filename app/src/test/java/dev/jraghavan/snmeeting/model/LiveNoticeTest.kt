package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveNoticeTest { // F15-FR12
    private fun settings(recorder: String, shown: String) = (CoreJson.obj("""{"settings":{"recorder":"$recorder",
        "liveNoticeShown":{$shown},"noticeShown":{"openai":true}},
        "effective":{"transcribe":{"provider":"openai"},"summarise":{"provider":"openai"}},"keys":{}}""", SettingsView::from) as CoreResult.Ok).value

    @Test fun theFirstNearLiveRecordingToAProviderNamesIt() {
        val first = LiveNotice.of(settings("near-live", ""))!!
        assertTrue(first.startsWith("Audio is sent to OpenAI while you record, about twice a minute, using your key."))
        assertNull(LiveNotice.of(settings("near-live", "\"openai\":true")))
        assertNull(LiveNotice.of(settings("standard", ""))) // the Standard recorder sends nothing while recording
        assertEquals(mapOf("openai" to true, "gemini" to false, "anthropic" to false), settings("near-live", "\"openai\":true").liveNoticeShown)
        assertEquals("""{"liveNoticeShown":{"gemini":true}}""", SettingsPatch.liveNoticeShown("gemini"))
    }
}
