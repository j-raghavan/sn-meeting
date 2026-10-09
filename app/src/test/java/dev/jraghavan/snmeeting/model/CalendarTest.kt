package dev.jraghavan.snmeeting.model

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.ZoneId

/** F18-FR7 / FR13 / FR14: what layout A and Settings → Calendar say, from the core's replies. */
class CalendarTest {
    private val la = ZoneId.of("America/Los_Angeles")
    /** 2026-10-09 10:00 PDT. */
    private val ten = 1_791_565_200_000L
    private val min = 60_000L

    private val agendaJson = """{"hasLinks":true,"today":"2026-10-09","nowMs":$ten,"updatedAtMs":$ten,"failed":["0123abcd"],
        "days":[{"day":"2026-10-09","dot":true,
          "allDay":[{"kind":"event","eventKey":"k:offsite:-","title":"Offsite","startMs":$ten,"endMs":${ten + 60 * min},"allDay":true,"people":"","marks":null}],
          "rows":[
            {"kind":"event","eventKey":"k:sync:x","title":"Weekly sync","startMs":$ten,"endMs":${ten + 30 * min},"allDay":false,
             "location":"Room 4","people":"Priya Shah, Ana Ruiz","calendar":"Work","recurring":true,"current":true,
             "meetingId":"meeting-20261009-100100","marks":{"recording":true,"notes":false,"minutes":true,"todosOpen":2,"todosTotal":3},
             "fromLastTime":[{"meetingId":"meeting-20261002-100000","meetingStartedAt":"2026-10-02T10:00:00-07:00","meetingTitle":"Weekly sync","todoId":"t1","text":"Send the deck","owner":null,"due":null}]},
            {"kind":"meeting","meetingId":"meeting-20261009-140000","title":null,"startMs":${ten + 240 * min},"endMs":${ten + 270 * min},"unscheduled":true,
             "marks":{"recording":true,"notes":false,"minutes":false,"todosOpen":0,"todosTotal":1}}
          ]},
          {"day":"2026-10-10","dot":false,"allDay":[],"rows":[]},
          {"day":"bad"}]}"""

    @Test
    fun the_agenda_reads() {
        val a = Agenda.from(JSONObject(agendaJson))
        assertTrue(a.hasLinks)
        assertEquals(1, a.failed)
        assertEquals(2, a.days.size)
        val d = a.day(LocalDate.of(2026, 10, 9))!!
        assertEquals(1, d.allDay.size)
        val sync = d.rows[0]
        assertTrue(sync.isEvent && sync.current && sync.recurring)
        assertEquals(Marks(true, false, true, 2, 3), sync.marks)
        assertEquals("Send the deck", sync.fromLastTime.single().text)
        assertEquals(sync, CalendarFormat.current(d.rows))
        val rec = d.rows[1]
        assertFalse(rec.isEvent)
        assertNull(CalendarFormat.rowTitle(rec))
        assertEquals("Unscheduled · tap to link", CalendarFormat.meetingLine(rec, hasLinks = true))
        assertEquals("no links: nothing to link to", "", CalendarFormat.meetingLine(rec, hasLinks = false))
        assertEquals(Marks.NONE, d.allDay[0].marks)
        assertNull(a.day(LocalDate.of(2026, 10, 11)))
    }

    @Test
    fun rows_and_marks_say_what_the_mock_up_says() {
        val d = Agenda.from(JSONObject(agendaJson)).days[0]
        val sync = d.rows[0]
        assertEquals("10:00–10:30", CalendarFormat.range(sync.startMs, sync.endMs, la))
        assertEquals("10:00", CalendarFormat.range(ten, ten, la))
        assertEquals("Room 4 · Priya Shah, Ana Ruiz", CalendarFormat.detail(sync))
        assertEquals("Weekly sync", CalendarFormat.rowTitle(sync))
        assertEquals(listOf("●" to true, "✎" to false, "≡" to true, "☐ 2" to false), CalendarFormat.marks(sync.marks))
        // No open to-dos: the slot stays empty.
        assertNull(CalendarFormat.marks(Marks(false, false, false, 0, 3))[3])
        val untitled = sync.copy(isEvent = false, unscheduled = false, title = " ")
        assertNull(CalendarFormat.rowTitle(untitled))
        val titled = untitled.copy(title = "Budget", durationMs = 38 * 60_000L, cost = "≈ \$0.01")
        assertEquals("Budget", CalendarFormat.rowTitle(titled))
        assertEquals("0:38 · \$0.01", CalendarFormat.meetingLine(titled, hasLinks = true))
        assertEquals("1:05", CalendarFormat.length(65 * 60_000L))
        assertEquals("0:01", CalendarFormat.length(5_000L))
        assertEquals("0:00", CalendarFormat.length(0L))
        assertEquals("(No title)", CalendarFormat.rowTitle(sync.copy(title = null)))
        assertEquals("", CalendarFormat.detail(sync.copy(location = null, people = "")))
    }

    @Test
    fun days_weeks_and_the_bottom_row() {
        val thu = LocalDate.of(2026, 10, 8)
        val fri = LocalDate.of(2026, 10, 9)
        assertEquals("Fri 9 Oct", CalendarFormat.headerDate(fri))
        assertEquals("Today", CalendarFormat.dayTitle(fri, fri))
        assertEquals("Yesterday", CalendarFormat.dayTitle(thu, fri))
        assertEquals("Tomorrow", CalendarFormat.dayTitle(fri.plusDays(1), fri))
        assertEquals("Mon 5 Oct", CalendarFormat.dayTitle(LocalDate.of(2026, 10, 5), fri))
        val w = CalendarFormat.week(LocalDate.of(2026, 10, 11))
        assertEquals(LocalDate.of(2026, 10, 5), w.first())
        assertEquals(LocalDate.of(2026, 10, 11), w.last())
        assertEquals("MON", CalendarFormat.weekday(w.first()))
        assertEquals("Updated 10:00", CalendarFormat.updated(ten, 0, la))
        assertEquals("Updated 10:00 · 1 failed", CalendarFormat.updated(ten, 1, la))
        assertEquals("Not updated yet", CalendarFormat.updated(null, 0, la))
    }

    @Test
    fun redraw_only_when_the_current_event_can_change() {
        val d = Agenda.from(JSONObject(agendaJson)).days[0]
        assertEquals(ten + 30 * min, CalendarFormat.nextChange(d.rows, ten + 1))
        assertNull("meetings and all-day rows never box", CalendarFormat.nextChange(d.rows, ten + 31 * min))
    }

    @Test
    fun the_link_picker_puts_overlapping_events_first() {
        fun ev(k: String, s: Long, e: Long) = AgendaRow(true, k, null, k, s, e, false, null, "", null, false, false, false, Marks.NONE, emptyList())
        val early = ev("early", ten - 120 * min, ten - 60 * min)
        val over = ev("over", ten, ten + 60 * min)
        val late = ev("late", ten + 120 * min, ten + 180 * min)
        val meeting = early.copy(isEvent = false, eventKey = null)
        assertEquals(listOf("over", "early", "late"),
            CalendarFormat.pickerOrder(listOf(late, early, over, meeting), ten + 5 * min, ten + 20 * min).map { it.eventKey })
    }

    @Test
    fun settings_lines_and_patches() {
        val l = CalendarLink.from(JSONObject("""{"linkId":"0123abcd","name":"Work","elided":"https://calendar.google.com/….ics",
            "lastOk":$ten,"status":"ok","reason":null,"eventsThisWeek":5,"skipped":2}"""))!!
        assertEquals("OK · updated Fri 9 Oct 10:00 · 5 events this week · 2 events couldn't be read", CalendarFormat.linkStatus(l, la))
        val gone = l.copy(status = "gone", reason = "This calendar link no longer works (404)", eventsThisWeek = 1, skipped = 0)
        assertEquals("This calendar link no longer works (404) · updated Fri 9 Oct 10:00 · 1 event this week", CalendarFormat.linkStatus(gone, la))
        assertEquals("Not fetched yet · 0 events this week", CalendarFormat.linkStatus(l.copy(status = "never", lastOk = null, eventsThisWeek = 0, skipped = 0), la))
        assertEquals("Failed · 0 events this week", CalendarFormat.linkStatus(l.copy(status = "failed", reason = null, lastOk = null, eventsThisWeek = 0, skipped = 0), la))
        assertNull(CalendarLink.from(JSONObject("{}")))
        assertEquals("""{"calendar":{"refreshMin":60}}""", CalendarPatch.refreshMin(60))
        assertEquals("""{"calendar":{"linkByTime":false}}""", CalendarPatch.linkByTime(false))
        assertEquals("""{"calendar":{"eventInMinutes":true}}""", CalendarPatch.eventInMinutes(true))
        assertEquals("""{"calendar":{"templateRules":[{"keyword":"1:1","templateId":"one-on-one"}]}}""",
            CalendarPatch.rules(listOf("1:1" to "one-on-one")))
        val p = CalendarPrefs.from(JSONObject("""{"refreshMin":0,"linkByTime":false,"templateRules":[{"keyword":"daily","templateId":"standup"},{"x":1}]}"""))
        assertEquals(CalendarPrefs(0, false, true, listOf("daily" to "standup")), p)
        assertEquals(CalendarPrefs(), CalendarPrefs.from(null))
        assertNull(CalendarRefresh.intervalMs(0))
        assertEquals(1_800_000L, CalendarRefresh.intervalMs(30))
        assertFalse(CalendarRefresh.autoAllowed(meetingJob = true))
        assertTrue(CalendarRefresh.autoAllowed(meetingJob = false))
        assertTrue(CalendarFormat.HOW_TO.contains("MyStyle/Meetings/calendar-links.txt"))
    }

    @Test
    fun the_event_sheet_reads() {
        val s = EventSheet.from(JSONObject("""{"calendar":"Work","event":{"eventKey":"k:sync:x","title":"Weekly sync","startMs":$ten,"endMs":${ten + 30 * min},
            "location":"Room 4","attendees":[{"name":"Priya Shah","email":"p@example.com"},{"name":null,"email":"a@example.com"}],
            "agenda":"1. Budget","recurring":true,"tzUnknown":false},
            "template":{"id":"standup","keyword":"stand-up"},
            "series":[{"meetingId":"meeting-20261002-100000","title":"Weekly sync","startedAt":"2026-10-02T10:00:00-07:00","openTodos":1}],
            "fromLastTime":[],"linked":["meeting-20261009-100100"]}"""))!!
        assertEquals(listOf("Priya Shah", "a@example.com"), s.attendees)
        assertEquals("standup", s.templateId)
        assertEquals("stand-up", s.templateKeyword)
        assertEquals(1, s.series.single().openTodos)
        assertEquals(listOf("meeting-20261009-100100"), s.linked)
        assertNull(EventSheet.from(JSONObject("{}")))
    }

    @Test
    fun a_todo_names_its_meeting_and_date() {
        val t = TodoRef("m", "2026-10-07T21:17:06-07:00", "Weekly sync", "t", "Send it", null, "2026-10-09")
        assertEquals("Weekly sync · Wed 7 Oct · Due Fri 9 Oct", CalendarFormat.todoSource(t))
        assertEquals("Untitled meeting · Wed 7 Oct", CalendarFormat.todoSource(t.copy(meetingTitle = null, due = null)))
    }
}
