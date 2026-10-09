package dev.jraghavan.snmeeting.model

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.YearMonth

/** F16-FR5 / FR4 / FR6: reading the core's spend replies, and what the screens show of them. */
class SpendTest {
    @Test fun rowsAndTheMeetingCarryTheCoresLabel() {
        val json = """[{"id":"meeting-20261007-211706","startedAt":"2026-10-07T21:17:06-07:00","startedAtMs":1,
            "durationMs":2530000,"status":"done","failedAt":null,"title":"Q4","segmentCount":1,"openTodoCount":0,
            "source":"sidecar","folder":"/f","spend":{"usd":0.41,"unpriced":1,"records":3,"label":"≈ ${'$'}0.41+"}}]"""
        val m = (CoreJson.list(json, MeetingSummary::from) as CoreResult.Ok).value.single()
        assertEquals("≈ \$0.41+", m.cost)
        assertEquals("≈ \$0.41+", HomeRows.meeting(m).cost)
        val old = """[{"id":"meeting-20261007-211706","status":"recorded","source":"legacy"}]"""
        assertEquals("—", (CoreJson.list(old, MeetingSummary::from) as CoreResult.Ok).value.single().cost)
        val v = """{"meeting":{"id":"meeting-20261007-211706","status":"done"},"source":"sidecar","dir":"/d",
            "segmentPaths":[],"transcriptPath":null,"spend":{"label":"< ${'$'}0.01"}}"""
        assertEquals("< \$0.01", (CoreJson.obj(v, MeetingView::from) as CoreResult.Ok).value.cost)
    }

    @Test fun monthsForTheCore() {
        assertEquals("2026-10", Spend.key(YearMonth.of(2026, 10)))
        assertEquals("2025-11" to "2026-10", Spend.lastMonths(YearMonth.of(2026, 10)))
        assertEquals("2026-10" to "2026-10", Spend.lastMonths(YearMonth.of(2026, 10), 1))
        assertEquals("October 2026", Spend.monthName("2026-10"))
        assertEquals("junk", Spend.monthName("junk"))
        assertEquals("\$3.20", Spend.money(3.2))
        assertEquals("< \$0.01", Spend.money(0.004))
        assertEquals("\$0.00", Spend.money(0.0))
    }

    @Test fun theSettingsTableIsNewestFirstWithAProviderLineEach() {
        val json = """{"months":[
            {"month":"2026-09","usd":0.5,"meetings":1,"unpriced":1,"label":"September · ${'$'}0.50+ spent · 1 meeting",
             "byProvider":{"openai":{"transcribe":0.5,"summarise":0.0}}},
            {"month":"2026-10","usd":3.2,"meetings":6,"unpriced":0,"label":"October · ${'$'}3.20 spent · 6 meetings",
             "byProvider":{"openai":{"transcribe":2.7,"summarise":0.0},"anthropic":{"transcribe":0.0,"summarise":0.5}}},
            {"month":"2026-11","usd":0,"meetings":0,"unpriced":0,"label":"November · nothing spent yet","byProvider":{}}],
            "unpriced":1,"notes":["Failed requests aren't counted"]}"""
        val r = (CoreJson.obj(json, SpendReport::from) as CoreResult.Ok).value
        assertEquals("October · \$3.20 spent · 6 meetings", r.months[1].label)
        val rows = r.rows()
        assertEquals(listOf("November 2026", "October 2026", "September 2026"), rows.map { it.month })
        assertEquals(listOf("—", "\$3.20", "\$0.50+"), rows.map { it.total })
        assertEquals(
            listOf("Anthropic  ·  Transcription \$0.00  ·  Minutes \$0.50", "OpenAI  ·  Transcription \$2.70  ·  Minutes \$0.00"),
            rows[1].lines,
        )
        assertEquals(listOf("Failed requests aren't counted"), r.notes)
        assertEquals(1, r.unpriced)
    }

    @Test fun handwrittenQueriesShowAsSearch() {
        val json = """{"months":[{"month":"2026-10","usd":0.003,"meetings":0,"unpriced":0,
            "label":"October · < ${'$'}0.01 spent · 0 meetings","byProvider":{},"search":0.003,"searches":2}],
            "unpriced":0,"notes":[]}"""
        val r = (CoreJson.obj(json, SpendReport::from) as CoreResult.Ok).value
        val row = r.rows().single()
        assertEquals("< \$0.01", row.total)
        assertEquals(listOf("Search  ·  < \$0.01"), row.lines)
    }

    private val rates = """{"asOf":"2026-11-02","problems":["Your rate for gpt-x has an error; using the default"],"entries":[
        {"provider":"openai","model":"whisper-1","unit":"minute","perMinute":0.006,"asOf":"2026-10-08","source":"https://x","origin":"default"},
        {"provider":"gemini","model":"gemini-3.8-flash","unit":"tokens","input":1.5,"output":7.5,"validFrom":"2027-01-01",
         "asOf":"2026-11-02","source":"edited in Settings","note":"Check this rate","origin":"user"}]}"""

    @Test fun ratesAreListedWithTheirFields() {
        val v = (CoreJson.obj(rates, RatesView::from) as CoreResult.Ok).value
        assertEquals("2026-11-02", v.asOf)
        assertEquals(1, v.problems.size)
        val (w, g) = v.entries
        assertEquals("OpenAI  ·  whisper-1", w.title)
        assertEquals(listOf("perMinute"), w.fields.map { it.first })
        assertEquals(0.006, w.price("perMinute")!!, 0.0)
        assertTrue(!w.user && g.user)
        assertEquals("Google Gemini  ·  gemini-3.8-flash  ·  from 2027-01-01", g.title)
        assertEquals("input", g.fields.first().first)
        assertNull(g.price("cachedInput"))
        assertEquals("Check this rate", g.note)
    }

    @Test fun anEditIsSentWholeAndJunkIsCaughtFirst() {
        val v = (CoreJson.obj(rates, RatesView::from) as CoreResult.Ok).value
        val (json, bad) = RatesEdit.build(v.entries, mapOf((0 to "perMinute") to " 0.007 ", (1 to "cachedInput") to "", (1 to "output") to "8"))
        assertNull(bad)
        val sent = JSONArray(json)
        assertEquals(2, sent.length())
        val w = sent.getJSONObject(0)
        assertEquals(0.007, w.getDouble("perMinute"), 0.0)
        assertTrue(!w.has("origin"))
        assertEquals(8.0, sent.getJSONObject(1).getDouble("output"), 0.0)
        assertEquals(1.5, sent.getJSONObject(1).getDouble("input"), 0.0) // untouched
        val (none, why) = RatesEdit.build(v.entries, mapOf((0 to "perMinute") to "cheap"))
        assertNull(none)
        assertEquals("OpenAI  ·  whisper-1: \"cheap\" isn't a number", why)
        val (cleared, _) = RatesEdit.build(v.entries, mapOf((1 to "input") to " "))
        assertTrue(!JSONArray(cleared).getJSONObject(1).has("input"))
        assertEquals("[]", RatesEdit.RESET)
    }

    @Test fun theRepriceQuestion() { // F16-FR6: how many meetings change, old and new totals
        val r = Reprice.from(JSONObject("""{"meetings":2,"written":0,"oldUsd":0.45,"newUsd":1.61,"unpricedBefore":1,"unpricedAfter":0}"""))
        assertEquals(
            "2 meetings will change. Their recorded total goes from \$0.45+ to \$1.61. The new rates replace what was recorded.",
            r.question(),
        )
        assertNull(Reprice.from(JSONObject("""{"meetings":0,"oldUsd":0,"newUsd":0}""")).question())
        assertTrue(Reprice.from(JSONObject("""{"meetings":1,"oldUsd":0.1,"newUsd":0.2,"unpricedAfter":2}""")).question()!!.contains("\$0.20+"))
    }

    @Test fun theMeetingHeaderCarriesTheCost() {
        val sc = Sidecar.from(JSONObject("""{"id":"meeting-20261007-211706","status":"done"}"""))!!
        assertEquals("≈ \$0.41", MeetingHeader.of(sc, "≈ \$0.41").cost)
        assertEquals("—", MeetingHeader.of(sc).cost)
    }
}
