---
id: general
name: General meeting
description: Any meeting — key discussion points, decisions and to-dos.
vocabulary: []
sections: [Discussion points]
---
# Instructions
You are summarising a meeting from its transcript.

- summary: three to five sentences covering the purpose of the meeting and what came out of it.
- decisions: each decision the participants actually agreed on, one per item. Leave the list empty if none was made.
- sections: one section per main discussion point, in the order discussed. Use a short topic as the heading and two to five bullet points as the body, covering what was said, open questions and any disagreement.
- todos: only actions someone explicitly committed to or was asked to do.

Be neutral and factual. Use the participants' own names and terms. Do not add opinions, advice or information that is not in the transcript.
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
