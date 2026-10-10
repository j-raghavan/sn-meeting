//! F19-FR4: the searchable units of a meeting (transcript lines, marks, recognised note lines,
//! minutes fields, to-dos) and of a calendar event. Built from the files alone (INV17).
use crate::notes_text::{Rect, Region};
use crate::sidecar::Sidecar;
use serde::{Deserialize, Serialize};

/// What a unit is; also the filter chips (F19-FR1) and the hit's source label (F19-FR2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Notes,
    Transcript,
    Minutes,
    Todo,
    Event,
    /// F20-FR5: a meeting's tag, one unit per tag.
    Tag,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Notes,
        Kind::Transcript,
        Kind::Minutes,
        Kind::Todo,
        Kind::Event,
        Kind::Tag,
    ];

    /// The id stored in the index.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Notes => "notes",
            Kind::Transcript => "transcript",
            Kind::Minutes => "minutes",
            Kind::Todo => "todo",
            Kind::Event => "event",
            Kind::Tag => "tag",
        }
    }

    pub fn from_id(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == s)
    }

    /// The source label a hit shows (F19-FR2).
    pub fn label(self) -> &'static str {
        match self {
            Kind::Notes => "NOTES",
            Kind::Transcript => "TRANSCRIPT",
            Kind::Minutes => "MINUTES",
            Kind::Todo => "TO-DO",
            Kind::Event => "EVENT",
            Kind::Tag => "TAG",
        }
    }
}

/// One searchable document (F19-FR4's `{kind, meetingId|eventKey, unitId, atMs, region, bbox,
/// text}`; the meeting or event is the unit's group).
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub kind: Kind,
    /// Stable within its group: `t:<segment>:<line>`, `mark:<id>`, `n:<region>:<line>`,
    /// `summary`, `decision:<i>`, `section:<i>`, `todo:<id>`, `event`, `tag:<name>`.
    pub unit_id: String,
    /// Recording-clock ms for timed units (transcript lines, marks, timed note lines).
    pub at_ms: Option<u64>,
    pub region: Option<u32>,
    /// A note line's world box.
    pub bbox: Option<Rect>,
    pub stroke_ids: Vec<u32>,
    /// A note line in a region written on since it was recognised (F19-FR7).
    pub changed: bool,
    pub text: String,
}

impl Unit {
    fn new(kind: Kind, unit_id: String, text: String) -> Unit {
        Unit {
            kind,
            unit_id,
            at_ms: None,
            region: None,
            bbox: None,
            stroke_ids: Vec::new(),
            changed: false,
            text: clean(&text),
        }
    }
}

/// F21-FR15: typed text (text boxes, sticky notes, table cells) as `Notes` units, indexed
/// directly with no provider call: `t:<canvas>:<itemId>[:<row>:<col>]`, timed by `recMs`, with
/// its world box.
pub fn typed_units(typed: &[crate::canvas::text::Typed]) -> Vec<Unit> {
    typed
        .iter()
        .map(|t| {
            let mut u = Unit::new(Kind::Notes, t.unit_id.clone(), t.text.clone());
            u.at_ms = t.at_ms;
            u.bbox = Some(t.bbox);
            u
        })
        .collect()
}

/// F21-FR15: the canvas a `Notes` unit is on, from its id (`t:<canvas>:…`, `n:…:c<canvas>`);
/// `None` for the first canvas or another kind.
pub fn canvas_of(kind: Kind, unit_id: &str) -> Option<u32> {
    if kind != Kind::Notes {
        return None;
    }
    let n = if let Some(rest) = unit_id.strip_prefix("t:") {
        rest.split(':').next()?.parse().ok()?
    } else {
        unit_id.rsplit_once(":c")?.1.parse().ok()?
    };
    (n != 1).then_some(n)
}

/// Control characters (the snippet markers among them) never reach the index.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Texts that stand for no speech are not searchable transcript.
fn is_placeholder(t: &str) -> bool {
    t == crate::providers::NO_SPEECH || t == crate::pipeline::AUDIO_MISSING
}

/// A segment's text as sentence lines, each with its character offset (F15-FR6's lines).
pub fn sentence_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 0usize; // char offset of the current line
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let ends = c == '\n'
            || (matches!(c, '.' | '!' | '?' | '…')
                && chars.get(i + 1).is_none_or(|n| n.is_whitespace()));
        if c != '\n' {
            cur.push(c);
        }
        if ends {
            push_line(&mut out, start, &cur);
            cur.clear();
            start = i + 1;
        }
    }
    push_line(&mut out, start, &cur);
    out
}

fn push_line(out: &mut Vec<(usize, String)>, start: usize, s: &str) {
    let lead = s.chars().take_while(|c| c.is_whitespace()).count();
    let t = s.trim();
    if !t.is_empty() {
        out.push((start + lead, t.to_owned()));
    }
}

/// The units of one meeting: its sidecar and its recognised notes. `changed` names the regions
/// written on since recognition (F19-FR7).
pub fn meeting_units(sc: &Sidecar, notes: &[Region], changed: &[u32]) -> Vec<Unit> {
    let mut out = Vec::new();
    // F15-FR6: a near-live meeting's transcript is its chunks', indexed as each lands; its
    // segments' text is the same words again, so it is not indexed twice.
    for p in crate::chunk::paragraphs(sc) {
        let Some(c) = sc.chunks.iter().find(|c| c.index == p.index) else {
            continue;
        };
        if is_placeholder(p.text.trim()) {
            continue;
        }
        for (li, (at, line)) in crate::chunk::lines(&p, c).into_iter().enumerate() {
            let mut u = Unit::new(Kind::Transcript, format!("t:c{}:{li}", p.index), line);
            u.at_ms = Some(at);
            out.push(u);
        }
    }
    let segments = if sc.chunks.is_empty() {
        &sc.segments[..]
    } else {
        &[]
    };
    for (si, seg) in segments.iter().enumerate() {
        let Some(text) = seg.text.as_deref().map(str::trim) else {
            continue;
        };
        if text.is_empty() || is_placeholder(text) {
            continue;
        }
        let total = text.chars().count().max(1) as u64;
        for (li, (off, line)) in sentence_lines(text).into_iter().enumerate() {
            // F15-FR6: the line's time, interpolated by character position (an estimate).
            let within = seg.duration_ms.map_or(0, |d| d * off as u64 / total);
            let mut u = Unit::new(Kind::Transcript, format!("t:{si}:{li}"), line);
            u.at_ms = Some(seg.start_offset_ms + within);
            out.push(u);
        }
    }
    for m in marks(sc) {
        let mut u = Unit::new(Kind::Transcript, format!("mark:{}", m.id), m.label);
        u.at_ms = Some(m.at_ms);
        out.push(u);
    }
    for r in notes {
        for (li, l) in r.lines.iter().enumerate() {
            // F21-FR15: a line on another canvas than the first names it.
            let id = if r.canvas == 1 {
                format!("n:{}:{li}", r.region)
            } else {
                format!("n:{}:{li}:c{}", r.region, r.canvas)
            };
            let mut u = Unit::new(Kind::Notes, id, l.text.clone());
            u.at_ms = l.at_ms;
            u.region = Some(r.region);
            u.bbox = Some(l.bbox);
            u.stroke_ids = l.stroke_ids.clone();
            u.changed = changed.contains(&r.region);
            out.push(u);
        }
    }
    if let Some(s) = &sc.summary {
        out.push(Unit::new(Kind::Minutes, "summary".into(), s.clone()));
    }
    for (i, d) in sc.decisions.iter().enumerate() {
        out.push(Unit::new(Kind::Minutes, format!("decision:{i}"), d.clone()));
    }
    for (i, s) in sc.sections.iter().enumerate() {
        let text = format!("{}\n{}", s.heading, s.body);
        out.push(Unit::new(Kind::Minutes, format!("section:{i}"), text));
    }
    for t in &sc.todos {
        out.push(Unit::new(
            Kind::Todo,
            format!("todo:{}", t.id),
            t.text.clone(),
        ));
    }
    // F20-FR5: each tag is its own unit, `tag:<name>`; the hit opens the Tag Graph.
    for t in &sc.tags {
        out.push(Unit::new(
            Kind::Tag,
            format!("tag:{}", t.name),
            t.name.clone(),
        ));
    }
    out.retain(|u| !u.text.is_empty());
    out
}

/// A labelled mark (F15): `marks` is an optional sidecar field, read leniently.
struct Mark {
    id: String,
    at_ms: u64,
    label: String,
}

fn marks(sc: &Sidecar) -> Vec<Mark> {
    let Some(list) = sc.extra.get("marks").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|m| {
            Some(Mark {
                id: m.get("id")?.as_str()?.to_owned(),
                at_ms: m.get("atMs")?.as_u64()?,
                label: m.get("label")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// A calendar event as Search needs it (F18's event cache; M13 fills the cache).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDoc {
    /// `<linkId>:<uid>:<recurrenceId|->` (F18-FR2).
    pub event_key: String,
    #[serde(default)]
    pub title: String,
    /// RFC 3339.
    pub start: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub agenda: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attendee {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// An event's one unit: title, location, attendees and agenda (F19-FR4).
pub fn event_units(e: &EventDoc) -> Vec<Unit> {
    let mut parts = vec![e.title.clone()];
    parts.extend(e.location.clone());
    parts.extend(
        e.attendees
            .iter()
            .filter_map(|a| a.name.clone().or_else(|| a.email.clone())),
    );
    parts.extend(e.agenda.clone());
    let text = parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let u = Unit::new(Kind::Event, "event".into(), text);
    if u.text.is_empty() { vec![] } else { vec![u] }
}

#[cfg(test)]
mod tests {
    /// F21-FR15: a Notes unit names its canvas when it isn't the first.
    #[test]
    fn notes_units_name_their_canvas() {
        assert_eq!(canvas_of(Kind::Notes, "t:2:abc"), Some(2));
        assert_eq!(canvas_of(Kind::Notes, "t:1:abc"), None);
        assert_eq!(canvas_of(Kind::Notes, "n:4:0:c3"), Some(3));
        assert_eq!(canvas_of(Kind::Notes, "n:4:0"), None);
        assert_eq!(canvas_of(Kind::Transcript, "t:2:0"), None);
        assert_eq!(canvas_of(Kind::Notes, "t:x:1"), None);
        let r = Region {
            region: 4,
            rect: [0.0; 4],
            lines: vec![crate::notes_text::Line {
                text: "on two".into(),
                bbox: [0.0; 4],
                stroke_ids: vec![],
                at_ms: None,
                extra: serde_json::Map::new(),
            }],
            canvas: 2,
            extra: serde_json::Map::new(),
        };
        let sc: Sidecar = serde_json::from_value(serde_json::json!({"schema": 1,
            "id": "meeting-20261009-100000", "startedAt": "2026-10-09T10:00:00-07:00",
            "status": "done"}))
        .unwrap();
        let u = meeting_units(&sc, &[r], &[]);
        assert_eq!(u[0].unit_id, "n:4:0:c2");
    }

    use super::*;

    #[test]
    fn kinds_round_trip() {
        for k in Kind::ALL {
            assert_eq!(Kind::from_id(k.id()), Some(k));
        }
        assert_eq!(Kind::from_id("x"), None);
        let labels = Kind::ALL.map(Kind::label);
        assert_eq!(
            labels,
            ["NOTES", "TRANSCRIPT", "MINUTES", "TO-DO", "EVENT", "TAG"]
        );
    }

    #[test]
    fn sentences_split_with_offsets() {
        let l = sentence_lines("Hello there. Q3 budget? v1.2 ships\n\n  ok… done");
        let texts: Vec<&str> = l.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            texts,
            ["Hello there.", "Q3 budget?", "v1.2 ships", "ok…", "done"]
        );
        assert_eq!(l[0].0, 0);
        assert_eq!(l[1].0, 13);
        assert_eq!(l[3].0, 38);
        assert!(sentence_lines("  ").is_empty());
    }

    #[test]
    fn control_characters_are_cleaned() {
        assert_eq!(clean("a\u{2}b\u{3}\tc\nd "), "a b  c\nd");
    }
}
