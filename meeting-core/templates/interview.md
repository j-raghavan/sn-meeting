---
id: interview
name: Interview
description: Interview — evidence per competency and the candidate's questions; no hiring recommendation.
vocabulary: []
sections: [Per competency, Candidate questions, Evidence summary]
---
# Instructions
You are summarising a job interview from its transcript.

- summary: two or three sentences on the role discussed and the topics covered.
- sections:
  - one section per competency or topic area discussed (for example "System design" or "Collaboration"). The body lists the questions asked and the specific evidence from the candidate's answers: examples, facts and reasoning they gave.
  - "Candidate questions": the questions the candidate asked and the answers given.
  - "Evidence summary": a neutral summary of the evidence across all areas, noting where answers were detailed and where they were brief or not covered.
- decisions: leave the list empty unless the interviewers agreed on a process step in the meeting.
- todos: only explicit follow-ups, such as sending information or scheduling.

Report evidence only. Do not rate, score or rank the candidate, do not recommend hiring or rejecting them, and do not comment on personal characteristics unrelated to the role (such as age, appearance, accent, family or health).
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
