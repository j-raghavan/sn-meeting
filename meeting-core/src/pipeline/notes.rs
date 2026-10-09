//! F19-FR6: the handwriting part of the Stop call. The first [`MAX_REGIONS_PER_CALL`] regions
//! ride on the minutes request (one call, `notes` schema field); any further regions go in
//! notes-only calls of the same size, each recorded in F16 like any other. The lines read back
//! are mapped to world space and to the strokes they cover, for `notes-text.json`.
use super::{JobPlan, Spent};
use crate::jobs::{self, Cancel};
use crate::keyfile::ProviderKey;
use crate::notes_text::{self, MAX_REGIONS_PER_CALL, NotesInput, Read, Region};
use crate::providers::{self, ImageIn, JsonAsk};
use crate::spend::Kind;
use serde_json::json;

/// The images for regions `from..` of one call, each labelled with its region number.
pub(crate) fn images(input: &NotesInput, from: usize) -> Vec<ImageIn> {
    input
        .regions
        .iter()
        .enumerate()
        .skip(from)
        .take(MAX_REGIONS_PER_CALL)
        .map(|(i, r)| ImageIn {
            label: format!("Note region {i}:"),
            png: r.png.clone(),
        })
        .collect()
}

/// What recognition produced: every region sent, and whether every call succeeded (only then
/// is the canvas recognised up to now, F19-FR7).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Recognised {
    pub regions: Vec<Region>,
    pub complete: bool,
}

/// The system prompt of a notes-only call.
pub const NOTES_ONLY_SYSTEM: &str =
    "You read handwritten notes. Reply only with JSON that matches the provided schema.";

/// A notes-only call's schema: just F19-FR6's `notes` field.
pub fn notes_only_schema() -> serde_json::Value {
    json!({"title": "meeting_notes", "type": "object", "additionalProperties": false,
           "required": ["notes"], "properties": {"notes": notes_text::schema_field()}})
}

/// The regions in `[from, to)` with the lines `reads` gave them (none when a region's reply is
/// missing, EC30), mapped onto the canvas.
fn mapped(input: &NotesInput, from: usize, to: usize, reads: &[Read]) -> Vec<Region> {
    (from..to)
        .map(|i| {
            let n = u32::try_from(i).unwrap_or(u32::MAX);
            let lines: Vec<(String, notes_text::Rect)> = reads
                .iter()
                .filter(|(r, _)| *r == n)
                .flat_map(|(_, l)| l.iter().cloned())
                .collect();
            notes_text::map_region(n, input.regions[i].rect, &lines, &input.strokes)
        })
        .collect()
}

/// F19-FR6: the first call's reply `text` gives regions 0..6; the rest are asked for now, six
/// at a time. A failed notes-only call leaves its regions out and the result incomplete; the
/// minutes are kept either way.
pub(crate) async fn recognise(
    plan: &JobPlan,
    key: &ProviderKey,
    model: &str,
    input: &NotesInput,
    first_reply: &str,
    spent: &Spent<'_>,
    cancel: &Cancel,
) -> Recognised {
    let total = input.regions.len();
    let first = total.min(MAX_REGIONS_PER_CALL);
    let mut regions = mapped(input, 0, first, &notes_text::parse_reply(first_reply));
    let mut complete = true;
    let mut from = first;
    while from < total {
        let to = (from + MAX_REGIONS_PER_CALL).min(total);
        let ask = JsonAsk {
            system: format!("{NOTES_ONLY_SYSTEM}\n{}", notes_text::NOTES_RULE),
            user: "The user's handwritten notes follow, one image per region.".into(),
            schema: notes_only_schema(),
            max_tokens: crate::summarise::MAX_OUTPUT_TOKENS,
            images: images(input, from),
        };
        let f = || {
            let fut = providers::complete_json(&plan.http, &plan.endpoints, key, model, &ask);
            async move {
                let r = fut.await;
                spent.note(Kind::Summarise, None, &r);
                r
            }
        };
        match jobs::with_retry(cancel, plan.budgets.summarise, f).await {
            Ok(r) => regions.extend(mapped(input, from, to, &notes_text::parse_reply(&r.text))),
            Err(_) => complete = false,
        }
        from = to;
    }
    Recognised { regions, complete }
}
