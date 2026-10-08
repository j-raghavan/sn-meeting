---
id: standup
name: Stand-up
description: Daily stand-up — per person: done, next, blockers.
vocabulary: []
sections: [Per person, Blockers]
---
# Instructions
You are summarising a daily stand-up from its transcript.

- summary: one or two sentences on the team's overall state.
- sections: one section per person who gave an update, in the order they spoke. The heading is the person's name. The body has three bullet lines: "Done:", "Next:" and "Blockers:" (write "none" when they reported none).
- Add a final section headed "Blockers" that lists every blocker raised, with who raised it and who offered to help. Write "None raised." when there were none.
- decisions: only decisions the team agreed on.
- todos: only explicit commitments, including offers to help unblock someone.

Keep each update short and in the speaker's own terms. If a speaker's name is not said, use "Speaker 1", "Speaker 2" and so on.
# Layout
# {{ title or "Untitled meeting" }}
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
{% endfor %}
