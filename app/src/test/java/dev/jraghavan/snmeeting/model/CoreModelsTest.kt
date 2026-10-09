package dev.jraghavan.snmeeting.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CoreModelsTest {
    @Test fun listRecentParsesNullsAsNull() {
        val json = """[{"id":"meeting-20261007-211706","startedAt":"2026-10-07T21:17:06-07:00","startedAtMs":1791000000000,
            "durationMs":null,"status":"recorded","failedAt":null,"title":null,"segmentCount":1,"openTodoCount":0,
            "source":"legacy","folder":"/storage/emulated/0/EXPORT/sn-meeting"}]"""
        val m = (CoreJson.list(json, MeetingSummary::from) as CoreResult.Ok).value.single()
        assertNull(m.title) // json.org's optString would give "", Android's "null": str() gives null on both
        assertNull(m.durationMs)
        assertEquals(Status.RECORDED, m.status)
        assertEquals(Source.LEGACY, m.source)
    }

    @Test fun errorEnvelopeCarriesFieldAndLine() {
        val r = CoreJson.obj("""{"error":"must be inside internal storage","field":"saveFolder"}""", SettingsView::from)
        assertEquals(CoreResult.Err("must be inside internal storage", "saveFolder", null), r)
        val t = CoreJson.obj("""{"error":"unexpected end of block","line":12,"field":"layout"}""") { it }
        assertEquals(12, (t as CoreResult.Err).line)
    }

    @Test fun garbageIsAnError() {
        assertTrue(CoreJson.list("not json", TodoRef::from) is CoreResult.Err)
        assertTrue(CoreJson.list("{\"a\":1}", TodoRef::from) is CoreResult.Err)
        assertTrue(CoreJson.ok("{\"ok\":true}") is CoreResult.Ok)
    }

    @Test fun openTodos() {
        val json = """[{"meetingId":"meeting-20261007-211706","meetingStartedAt":"2026-10-07T21:17:06-07:00",
            "meetingTitle":null,"todoId":"7f3a","text":"Send the deck","owner":"me","due":"2026-10-09"}]"""
        val t = (CoreJson.list(json, TodoRef::from) as CoreResult.Ok).value.single()
        assertEquals("7f3a", t.todoId)
        assertNull(t.meetingTitle)
    }

    @Test fun sidecarFields() {
        val v = """{"meeting":{"schema":1,"id":"meeting-20261007-211706","startedAt":"x","durationMs":2520000,
          "status":"transcribing","failedAt":null,"title":"Q4","titleSource":"auto","templateId":"general",
          "providers":{"transcribe":{"provider":"openai","model":"gpt-transcribe"},"summarise":null},
          "summary":null,"decisions":[],"sections":[{"heading":"Blockers","body":"none"}],
          "mic":{"kind":"usb","name":"USB-Audio - USB Audio"},
          "audio":{"codec":"aac-lc","sampleRateHz":44100,"bitrateBps":96000,"channels":1},
          "segments":[{"file":"a-01.m4a","startOffsetMs":0,"durationMs":900000,"text":"hi"},
                      {"file":"a-02.m4a","startOffsetMs":900100,"durationMs":900000,"text":null},
                      {"file":"a-03.m4a","startOffsetMs":1800200,"durationMs":0,"text":null,"flag":"empty"}],
          "transcriptFile":null,"error":null,"note":"mic disconnected",
          "todos":[{"id":"t1","text":"Send deck","owner":"me","due":null,"done":false,"firstSeen":"x"}],"futureField":7},
          "source":"sidecar","dir":"/storage/emulated/0/EXPORT/sn-meeting","segmentPaths":["/p/a-01.m4a"],"transcriptPath":null}"""
        val m = (CoreJson.obj(v, MeetingView::from) as CoreResult.Ok).value
        assertEquals("empty", m.meeting.segments[2].flag)
        assertNull(m.meeting.segments[1].flag)
        assertEquals(ProviderRun("openai", "gpt-transcribe"), m.meeting.transcribeRun)
        assertNull(m.meeting.summariseRun)
        assertNull(m.meeting.todos[0].due)
        assertNull(m.meeting.error) // a sidecar's own "error": null is not the core's error envelope
        assertEquals("mic disconnected", m.meeting.note)
        assertNull(m.transcriptPath)
    }

    @Test fun jobRepliesAndRecorderReplies() {
        val job = CoreJson.obj("""{"meeting":{"id":"m","status":"failed","failedAt":"transcribing","error":"No connection"}}""", Sidecar::ofJob)
        val sc = (job as CoreResult.Ok).value
        assertEquals(FailedAt.TRANSCRIBING, sc.failedAt)
        assertEquals("No connection", sc.error)
        assertEquals(CoreResult.Err("meeting is busy"), CoreJson.obj("""{"error":"meeting is busy"}""", Sidecar::ofJob))
        val b = CoreJson.obj("""{"id":"m","firstSegmentPath":"/d/m-01.m4a","plan":{"maxDurationMs":900000,"maxFileBytes":13499997}}""", Begun::from)
        assertEquals(Begun("m", "/d/m-01.m4a", 900_000, 13_499_997), (b as CoreResult.Ok).value)
        assertEquals(SegmentAdded(1, "/d/m-02.m4a"), CoreJson.obj("""{"index":1,"path":"/d/m-02.m4a"}""", SegmentAdded::from).getOrNull())
    }

    @Test fun recoveryNoAudioIsOptional() {
        val r = CoreJson.obj("""{"recovered":["a"],"measure":[{"id":"a","index":1,"path":"/p/a-02.m4a"}],"flagged":[]}""", Recovery::from).getOrNull()!!
        assertEquals(listOf(Recovery.ToMeasure("a", 1, "/p/a-02.m4a")), r.measure)
        assertEquals(emptyList<String>(), r.noAudio)
        val s = CoreJson.obj("""{"recovered":[],"measure":[],"flagged":[{"id":"b","index":0,"flag":"missing"}],"noAudio":["b"]}""", Recovery::from).getOrNull()!!
        assertEquals(listOf("b"), s.noAudio)
        assertEquals("missing", s.flagged.single().flag)
    }

    @Test fun updatePrefsAndPatches() { // F14-FR8
        val j = """{"settings":{"updates":{"autoCheck":false,"autoInstall":true,"lastCheckMs":1791000000000,"skipVersion":"0.2.0"}},"effective":{}}"""
        val u = (CoreJson.obj(j, SettingsView::from) as CoreResult.Ok).value.updates
        assertEquals(UpdatePrefs(autoCheck = false, autoInstall = true, lastCheckMs = 1_791_000_000_000L, skipVersion = "0.2.0"), u)
        assertFalse("auto-install acts only with auto-check", u.autoInstallEffective)
        // An older settings.json without the object: inkread's defaults.
        val old = (CoreJson.obj("""{"settings":{},"effective":{}}""", SettingsView::from) as CoreResult.Ok).value.updates
        assertEquals(UpdatePrefs(), old)
        assertTrue(old.autoCheck)
        assertFalse(old.autoInstall)
        assertEquals("""{"updates":{"autoCheck":false,"autoInstall":false}}""", SettingsPatch.autoCheck(false))
        assertEquals("""{"updates":{"autoCheck":true}}""", SettingsPatch.autoCheck(true))
        assertEquals("""{"updates":{"autoInstall":true,"autoCheck":true}}""", SettingsPatch.autoInstall(true))
        assertEquals("""{"updates":{"autoInstall":false}}""", SettingsPatch.autoInstall(false))
        assertEquals("""{"updates":{"lastCheckMs":42}}""", SettingsPatch.lastCheck(42))
        assertEquals("""{"updates":{"skipVersion":"0.2.0"}}""", SettingsPatch.skipVersion("0.2.0"))
    }

    @Test fun settingsViewAndPatches() {
        val j = """{"settings":{"schema":1,"mic":"bt:Jabra Evolve2","saveFolder":"EXPORT/sn-meeting","previousFolders":[],
          "transcribe":{"provider":null,"model":null},"summarise":{"provider":"anthropic","model":"claude-opus-5-5"},
          "defaultTemplate":"standup","noticeShown":{"openai":true,"gemini":false,"anthropic":false},"debug":{"segmentMs":null}},
          "origin":"file","effective":{"transcribe":{"provider":"openai","model":"gpt-transcribe","modelSource":"default"},
          "summarise":{"provider":"anthropic","model":"claude-opus-5-5","modelSource":"override"},
          "saveFolderPath":"/storage/emulated/0/EXPORT/sn-meeting","defaultTemplate":"general"},
          "keys":{"openai":{"status":"found","file":"copilot-key-openai.txt","model":"gpt-4o-mini","transcribeModel":null},
                  "gemini":{"status":"missing","file":"copilot-key-gemini.txt","detail":"Add copilot-key-gemini.txt to MyStyle/SnCopilot"},
                  "anthropic":{"status":"unreadable","detail":"Can't read MyStyle/SnCopilot"}}}"""
        val s = (CoreJson.obj(j, SettingsView::from) as CoreResult.Ok).value
        assertEquals(MicChoice.Bluetooth("Jabra Evolve2"), s.mic)
        assertTrue(s.keys["openai"]!!.usable)
        assertEquals("missing", (s.keys["gemini"] as KeyStatus.Problem).kind)
        assertEquals("", s.keys["anthropic"]!!.file)
        assertEquals(true, s.noticeShown["openai"])
        assertNull(s.transcribe.provider)
        assertNull(s.segmentMsDebug)
        assertEquals("general", s.effectiveDefaultTemplate) // the chosen one is gone
        assertEquals("override", s.effective("summarise").modelSource)
        assertEquals("""{"summarise":{"model":null}}""", SettingsPatch.model("summarise", " "))
        assertEquals("""{"mic":"usb:USB \"Audio\""}""", SettingsPatch.mic(MicChoice.Usb("USB \"Audio\"")))
        assertEquals("""{"noticeShown":{"openai":true,"anthropic":true}}""", SettingsPatch.noticeShown(listOf("openai", "anthropic")))
        assertEquals("""{"debug":{"segmentMs":60000}}""", SettingsPatch.segmentMs(60_000))
        assertEquals("""{"debug":{"segmentMs":null}}""", SettingsPatch.segmentMs(null))
    }

    @Test fun testResultReadsSnakeCase() {
        val r = (CoreJson.obj("""{"ok":true,"status":"connected","ms":312,"provider":"openai","model":"gpt-4o-mini","model_found":false,"detail":"Connected · 312 ms · model gpt-4o-mini not found"}""", TestResult::from) as CoreResult.Ok).value
        assertEquals(false, r.modelFound)
        assertEquals(312L, r.ms)
    }

    @Test fun templateRows() {
        val j = """[{"id":"standup","name":"Stand-up","description":"d","vocabulary":["Priya"],"sections":[],"instructions":"i",
            "layout":"l","origin":"customised","builtin":true,"customised":true,"problem":null,"path":"/s/standup.md"}]"""
        val t = (CoreJson.list(j, TemplateRow::from) as CoreResult.Ok).value.single()
        assertEquals(TemplateOrigin.CUSTOMISED, t.origin)
        assertEquals(listOf("Priya"), t.vocabulary)
        assertNull(t.problem)
    }

    @Test fun saveFolders() {
        assertTrue(SaveFolders.isCustom("Notes/Meetings"))
        assertEquals(false, SaveFolders.isCustom("EXPORT/sn-meeting"))
        assertNull(SaveFolders.previous(emptyList()))
        assertEquals("Meetings in EXPORT/sn-meeting stay listed.", SaveFolders.previous(listOf("EXPORT/sn-meeting")))
        assertEquals("""{"saveFolder":"Notes/Meetings"}""", SettingsPatch.saveFolder("Notes/Meetings"))
    }

    @Test fun providerRows() { // F10-FR4, F10-AC1
        val j = """{"settings":{"mic":"auto","saveFolder":"EXPORT/sn-meeting","transcribe":{},"summarise":{}},
          "effective":{"transcribe":{"provider":"openai","model":"gpt-transcribe","modelSource":"default"},
          "summarise":{"provider":"openai","model":"gpt-4o-mini","modelSource":"keyFile"}},
          "keys":{"openai":{"status":"found","file":"copilot-key-openai.txt","model":"gpt-4o-mini","transcribeModel":null},
                  "gemini":{"status":"missing","file":"copilot-key-gemini.txt","detail":"Add copilot-key-gemini.txt to MyStyle/SnCopilot"},
                  "anthropic":{"status":"missing","file":"copilot-key-anthropic.txt","detail":"Add copilot-key-anthropic.txt to MyStyle/SnCopilot"}}}"""
        val s = CoreJson.obj(j, SettingsView::from).getOrNull()!!
        val t = ProviderRows.of("transcribe", s)
        assertEquals(listOf("OpenAI", "Google Gemini"), t.map { it.name }) // Anthropic takes no audio
        assertEquals(ProviderRows.Row("openai", "OpenAI", "Key found: copilot-key-openai.txt", true, false), t[0])
        assertEquals(ProviderRows.Row("gemini", "Google Gemini", "Add copilot-key-gemini.txt to MyStyle/SnCopilot", false, false), t[1])
        assertEquals(3, ProviderRows.of("summarise", s).size)
        assertEquals("gpt-transcribe  ·  the default", ProviderRows.model("transcribe", s))
        assertEquals("gpt-4o-mini  ·  from the key file", ProviderRows.model("summarise", s))
        val auto = CoreJson.obj(j.replace(""""gpt-transcribe","modelSource":"default"""", """"gpt-4o-transcribe","modelSource":"auto""""), SettingsView::from).getOrNull()!!
        assertEquals("gpt-4o-transcribe  ·  auto", ProviderRows.model("transcribe", auto)) // D2
        // D2 before the account is listed: the first candidate, honestly not yet checked.
        val unchecked = CoreJson.obj(j.replace(""""modelSource":"default"""", """"modelSource":"autoUnchecked""""), SettingsView::from).getOrNull()!!
        assertEquals("gpt-transcribe  ·  auto (checked on first use)", ProviderRows.model("transcribe", unchecked))
        val whisper = CoreJson.obj(j.replace(""""gpt-transcribe","modelSource":"default"""", """"whisper-1","modelSource":"auto""""), SettingsView::from).getOrNull()!!
        assertEquals("whisper-1  ·  auto", ProviderRows.model("transcribe", whisper))
        assertEquals("""{"transcribe":{"provider":"gemini"}}""", SettingsPatch.provider("transcribe", "gemini"))
        assertEquals("""{"transcribe":{"model":"whisper-1"}}""", SettingsPatch.model("transcribe", " whisper-1 "))
    }

    @Test fun testConnectionLine() { // F10-FR5
        assertEquals("Key rejected (401)", TestResult.line("""{"ok":false,"status":"key_rejected","ms":80,"provider":"openai","model":"m","model_found":null,"detail":"Key rejected (401)"}"""))
        assertEquals("Connected", TestResult.line("""{"ok":true,"status":"connected","ms":80}"""))
        assertEquals("Couldn't check: core unavailable: UnsatisfiedLinkError", TestResult.line("""{"error":"core unavailable: UnsatisfiedLinkError"}"""))
    }
}
