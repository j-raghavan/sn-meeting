package dev.jraghavan.snmeeting

/**
 * Sample meetings for the layout mock-up of the home list (spec F6) and the meeting screen (F7).
 * Replaced by the Rust core's library (F3) — nothing here touches disk or the network.
 */
enum class MockStatus(val label: String, val attention: Boolean) {
    RECORDED("Recorded", false),
    TRANSCRIBING("Transcribing 2/3", false),
    DONE("Transcribed", false),
    FAILED("Failed", true),
}

class MockTodo(val text: String, val meta: String, var done: Boolean = false)

class MockMeeting(
    val id: String,
    /** From the extraction request (F5) or the user's rename; null until then. */
    val title: String?,
    val day: String,
    val dayLong: String,
    val time: String,
    val duration: String,
    val mic: String,
    val segments: Int,
    val status: MockStatus,
    val todos: List<MockTodo> = emptyList(),
    /** Paragraphs; the marker is the segment start (`[h:mm:ss]`), null for a paragraph continuing it. */
    val transcript: List<Pair<String?, String>> = emptyList(),
    val error: String? = null,
) {
    val openTodos get() = todos.count { !it.done }
    val displayTitle get() = title ?: "Untitled meeting"
}

object MockData {
    val meetings: List<MockMeeting> = listOf(
        MockMeeting(
            id = "m1", title = "Q4 planning & vendor shortlist", day = "Wed 7 Oct", dayLong = "Wednesday, 7 October", time = "14:02",
            duration = "0:42:10", mic = "USB — USB-Audio - USB Audio", segments = 3,
            status = MockStatus.DONE,
            todos = listOf(
                MockTodo("Send the Q4 deck to Priya", "Due Fri 9 Oct · Me"),
                MockTodo("Book the design review for next week", "No date · Arjun"),
                MockTodo("Share the vendor shortlist", "Due Mon 12 Oct · Me", done = true),
                MockTodo("Flag the holiday freeze in the deck", "Due Fri 9 Oct · Me"),
                MockTodo("Ask finance for the vendor budget cap", "No date · Priya"),
            ),
            transcript = listOf(
                "0:00:00" to "Okay, let's get started. The main thing today is the Q4 plan and the vendor decision. Priya, you wanted the deck before Friday, right? Yes, ideally Thursday evening so I can review it before the leadership sync.",
                null to "I'll send the Q4 deck to Priya by Friday at the latest. On the design side, Arjun, can you book the review for next week? We need everyone in the room for that one.",
                "0:20:00" to "Moving on to vendors. We narrowed it to three. The pricing on the second one is the most flexible, but their support hours don't cover our evenings. I'll share the shortlist with the team on Monday with the comparison table.",
                null to "Any concerns on timeline? Only that the migration overlaps with the holiday freeze. Let's flag that in the deck so leadership sees it early.",
                "0:40:00" to "Great, that's everything. Thanks all — same time next week.",
            ),
        ),
        MockMeeting(
            id = "m2", title = null, day = "Wed 7 Oct", dayLong = "Wednesday, 7 October", time = "10:30",
            duration = "0:18:45", mic = "Bluetooth — Jabra Evolve2", segments = 1,
            status = MockStatus.RECORDED,
        ),
        MockMeeting(
            id = "m3", title = null, day = "Tue 6 Oct", dayLong = "Tuesday, 6 October", time = "16:15",
            duration = "0:55:02", mic = "USB — USB-Audio - USB Audio", segments = 3,
            status = MockStatus.TRANSCRIBING,
        ),
        MockMeeting(
            id = "m4", title = null, day = "Mon 5 Oct", dayLong = "Monday, 5 October", time = "09:00",
            duration = "0:31:20", mic = "USB — USB-Audio - USB Audio", segments = 2,
            status = MockStatus.FAILED, error = "No connection — tap Transcribe to retry.",
        ),
        MockMeeting(
            id = "m5", title = "Onboarding review", day = "Fri 2 Oct", dayLong = "Friday, 2 October", time = "13:40",
            duration = "1:04:33", mic = "USB — USB-Audio - USB Audio", segments = 4,
            status = MockStatus.DONE,
            todos = listOf(
                MockTodo("Draft the onboarding checklist", "Due Thu 8 Oct · Me"),
                MockTodo("Pair each new joiner with a buddy", "No date · Arjun"),
                MockTodo("Share the laptop order list with IT", "Due Mon 5 Oct · Me"),
                MockTodo("Book the welcome lunch", "No date · Priya"),
            ),
            transcript = listOf(
                "0:00:00" to "Thanks for joining. Let's review onboarding. I'll draft the onboarding checklist by Thursday.",
            ),
        ),
    )

    fun byId(id: String?): MockMeeting? = meetings.firstOrNull { it.id == id }
}

/** Sample Settings state for the layout mock-up (spec F10). Held in memory only. */
object MockSettings {
    class Provider(val id: String, val name: String, val keyFile: String, val hasKey: Boolean, val model: String)

    /** Key status mirrors the device today: only copilot-key-openai.txt exists (spec F4). */
    val providers = listOf(
        Provider("openai", "OpenAI", "copilot-key-openai.txt", hasKey = true, model = "gpt-transcribe"),
        Provider("gemini", "Google Gemini", "copilot-key-gemini.txt", hasKey = false, model = "gemini-3.8-flash"),
        Provider("anthropic", "Anthropic", "copilot-key-anthropic.txt", hasKey = false, model = "claude-opus-5-5"),
    )
    val transcribeChoices = listOf("openai", "gemini") // Anthropic can't take audio
    val summariseChoices = listOf("openai", "gemini", "anthropic")

    var mic = "auto"
    var folder = "EXPORT/sn-meeting"
    var transcribe = "openai"
    var summarise = "openai"
    var defaultTemplate = "general"

    fun provider(id: String) = providers.first { it.id == id }
}

/** The six built-in templates (spec F11-FR2), with sample text for the editor mock-up. */
class MockTemplate(
    val id: String,
    var name: String,
    var description: String,
    var state: String, // "Built-in" | "Customised" | "Yours"
    var vocabulary: String,
    var instructions: String,
)

object MockTemplates {
    const val STANDARD_LAYOUT = """# {{ title or "Untitled meeting" }}
{{ weekday }} {{ date }} · {{ time }} · {{ duration }} · {{ template }}

## Summary
{{ summary }}
{% if decisions %}
## Decisions
{% for d in decisions %}- {{ d }}
{% endfor %}{% endif %}
{% for s in sections %}
## {{ s.heading }}
{{ s.body }}
{% endfor %}
## To-dos
{% for t in todos %}- [{{ "x" if t.done else " " }}] {{ t.text }}{% if t.owner %} — {{ t.owner }}{% endif %}{% if t.due %} (due {{ t.due }}){% endif %}
{% endfor %}
## Transcript
{% for p in transcript %}[{{ p.marker }}] {{ p.text }}
{% endfor %}"""

    const val SAMPLE_RENDER = """# Q4 planning & vendor shortlist
Wednesday 7 Oct 2026 · 14:02 · 0:42:10 · General meeting

## Summary
The team agreed the Q4 plan and narrowed vendors to three; the migration overlaps the holiday freeze.

## Decisions
- Shortlist three vendors; compare support hours before choosing.

## Key points
Pricing is most flexible with vendor two, but its support hours miss our evenings.

## To-dos
- [ ] Send the Q4 deck to Priya — Me (due 2026-10-09)
- [ ] Book the design review for next week — Arjun
- [x] Share the vendor shortlist — Me (due 2026-10-12)

## Transcript
[0:00:00] Okay, let's get started. The main thing today is the Q4 plan…
[0:15:00] Moving on to vendors. We narrowed it to three…"""

    val all = mutableListOf(
        MockTemplate("general", "General meeting", "Summary, decisions, key points and to-dos.", "Built-in", "",
            "You are summarising a work meeting from its transcript.\nGive a short title (at most 60 characters), a 2–4 sentence summary, the decisions made, and the key discussion points as sections.\nList to-dos only for explicit action items, commitments and follow-ups. Give each an owner if named, and a due date if one is stated."),
        MockTemplate("standup", "Stand-up", "Per person: done, next, blockers.", "Customised", "Priya, Arjun, Supernote",
            "You are summarising a daily stand-up.\nWrite one section per person with three short lines: Done, Next, Blockers.\nThen a Blockers section listing every blocker with who raised it.\nTo-dos only for explicit commitments."),
        MockTemplate("one-on-one", "1:1", "Wins, concerns, feedback and career topics.", "Built-in", "",
            "You are summarising a one-to-one meeting.\nUse sections for Wins, Concerns, Feedback given and received, and Career topics. Keep a neutral, private tone."),
        MockTemplate("client-call", "Client call", "Goals, requirements, objections, commitments.", "Built-in", "",
            "You are summarising a call with a client.\nUse sections for Client goals, Requirements, Objections, Commitments we made, and Next meeting."),
        MockTemplate("interview", "Interview", "Evidence by competency; no hiring verdict.", "Built-in", "",
            "You are summarising a job interview.\nWrite one section per competency discussed with the evidence heard, then the candidate's questions. Do not recommend hiring or not hiring."),
        MockTemplate("lecture", "Lecture / talk notes", "Topics, key points and definitions.", "Built-in", "",
            "You are taking notes on a lecture or talk.\nWrite one section per topic with key points and definitions. To-dos only for explicit follow-ups."),
    )

    fun byId(id: String?) = all.firstOrNull { it.id == id }
}
