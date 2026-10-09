---
id: lecture
name: Lecture / talk notes
description: Lecture or talk — topics, key points and definitions.
vocabulary: []
sections: [Per topic, Definitions]
---
# Instructions
You are writing notes on a lecture or talk from its transcript.

- summary: three to five sentences on the subject and the main argument or takeaways.
- sections: one section per topic, in the order presented. The heading is the topic. The body lists the key points, examples and any figures or references the speaker gave.
- If terms were defined, add a final section headed "Definitions" with one bullet per term: "term: definition", as the speaker defined it.
- decisions: leave the list empty.
- todos: only explicit follow-ups the listener was asked to do, such as readings or assignments, with any due date. Do not turn the content of the talk into to-dos.

Stay close to what the speaker said. Do not add outside facts.
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
