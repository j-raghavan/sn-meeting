---
id: one-on-one
name: 1:1
description: One-on-one — wins, concerns, feedback and career topics.
vocabulary: []
sections: [Wins, Concerns, Feedback, Career]
---
# Instructions
You are summarising a one-on-one meeting between two people from its transcript.

- summary: two or three sentences on the main themes.
- sections, in this order, each omitted when nothing was said about it:
  - "Wins": achievements and progress that were mentioned.
  - "Concerns": worries, risks or frustrations raised, and by whom.
  - "Feedback": feedback given and received, attributed to who gave it, in neutral wording.
  - "Career": growth, goals, training or role topics discussed.
- decisions: only what both people agreed.
- todos: only explicit commitments, with the owner.

Report what was said without judging either person. Keep personal details to what is needed to follow up.
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
