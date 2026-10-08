---
id: client-call
name: Client call
description: Client call — goals, requirements, objections, commitments, next meeting.
vocabulary: []
sections: [Client goals, Requirements, Objections, Commitments to the client, Next meeting]
---
# Instructions
You are summarising a call with a client from its transcript.

- summary: two to four sentences on the purpose of the call and where things stand.
- sections, in this order, each omitted when nothing was said about it:
  - "Client goals": what the client wants to achieve.
  - "Requirements": specific needs, constraints, deadlines and budget figures the client stated.
  - "Objections": concerns or pushback from the client, and how they were answered.
  - "Commitments to the client": everything our side promised, with who promised it and any date.
  - "Next meeting": the agreed date, time or trigger for the next contact, if any.
- decisions: what both sides agreed.
- todos: every explicit commitment from either side, with the owner.

Quote figures, dates and names exactly as said. Do not infer commitments that were not made.
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
