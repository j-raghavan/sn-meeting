//! F21-FR15: the typed text of a canvas: text boxes, sticky notes and table cells. It is
//! indexed directly as `Notes` search units (no provider call, no `notes-text.json` entry) and
//! goes to the minutes call as one text block, in reading order: canvas, then top to bottom.
use super::geom;
use super::scene::Scene;

/// The typed-notes block is capped at this many characters (F21-FR15).
pub const MAX_TYPED_CHARS: usize = 8_000;
/// The block's heading, as F21-FR15 words it.
pub const TYPED_HEADING: &str = "The user's typed notes:";

/// One piece of typed text.
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    pub canvas: u32,
    /// The search unit's id: `t:<canvas>:<itemId>[:<row>:<col>]`.
    pub unit_id: String,
    pub text: String,
    /// Its time on the recording clock (an item's `recMs`, a cell's own), or untimed.
    pub at_ms: Option<u64>,
    /// Its world box.
    pub bbox: [f64; 4],
}

/// The typed text of `scene` (canvas `canvas`), top to bottom then left to right.
#[must_use]
pub fn typed(scene: &Scene, canvas: u32) -> Vec<Typed> {
    let mut out = Vec::new();
    for it in &scene.doc().items {
        let b = geom::bounds_of(&geom::corners(it));
        let bbox = b.map_or([it.x, it.y, it.x + it.width, it.y + it.height], |b| {
            [
                f64::from(b.x0),
                f64::from(b.y0),
                f64::from(b.x1),
                f64::from(b.y1),
            ]
        });
        match it.kind.as_str() {
            "text" | "note" => {
                let t = it.text.as_deref().unwrap_or("").trim();
                if !t.is_empty() {
                    out.push(Typed {
                        canvas,
                        unit_id: format!("t:{canvas}:{}", it.id),
                        text: t.to_owned(),
                        at_ms: it.rec_ms,
                        bbox,
                    });
                }
            }
            "table" => {
                let Some(tb) = &it.table else { continue };
                let heights = tb.row_heights();
                let w = it.width / tb.cols as f64;
                for row in 0..tb.rows {
                    let top = it.y + heights[..row].iter().sum::<f64>();
                    for col in 0..tb.cols {
                        let i = row * tb.cols + col;
                        let t = tb.cells[i].trim();
                        if t.is_empty() {
                            continue;
                        }
                        #[allow(clippy::cast_precision_loss)]
                        let left = it.x + col as f64 * w;
                        out.push(Typed {
                            canvas,
                            unit_id: format!("t:{canvas}:{}:{row}:{col}", it.id),
                            text: t.to_owned(),
                            at_ms: tb.cell_rec_ms[i].or(it.rec_ms),
                            bbox: [left, top, left + w, top + heights[row]],
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| {
        (a.bbox[1], a.bbox[0])
            .partial_cmp(&(b.bbox[1], b.bbox[0]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The block the minutes call carries (F21-FR15), or `None` without typed text: the heading,
/// then one line per piece, capped at [`MAX_TYPED_CHARS`].
#[must_use]
pub fn block(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let mut out = String::from(TYPED_HEADING);
    for l in lines {
        let one = l.split_whitespace().collect::<Vec<_>>().join(" ");
        let add = format!("\n- {one}");
        if out.chars().count() + add.chars().count() > MAX_TYPED_CHARS {
            let room = MAX_TYPED_CHARS.saturating_sub(out.chars().count());
            out.extend(add.chars().take(room));
            break;
        }
        out.push_str(&add);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::item::{Item, Table};
    use crate::canvas::scene::tests::with_item;
    use crate::canvas::style::Style;
    use crate::ink::InkLayer;

    #[test]
    fn typed_text_in_reading_order_with_its_times() {
        let mut s = Scene::load(InkLayer::new(), Ok(None)).0;
        let mut low = Item::new("low", "text", Style::default());
        (low.y, low.width, low.height, low.rec_ms) = (500.0, 320.0, 40.0, Some(761_000));
        low.text = Some(" Budget  owner? ".into());
        let mut note = Item::new("n", "note", Style::default());
        (note.y, note.width, note.height) = (10.0, 200.0, 200.0);
        note.text = Some("Ask Sam".into());
        let mut blank = Item::new("b", "text", Style::default());
        blank.text = Some("  ".into());
        let mut tb = Item::new("tb", "table", Style::default());
        (tb.y, tb.width, tb.height, tb.rec_ms) = (100.0, 320.0, 96.0, Some(5));
        let mut t = Table::empty(2, 2);
        t.cells[3] = "Q3".into();
        t.cell_rec_ms[3] = Some(9);
        t.cells[0] = "Owner".into();
        tb.table = Some(t);
        for it in [
            low,
            note,
            blank,
            tb,
            Item::new("r", "rectangle", Style::default()),
        ] {
            with_item(&mut s, it);
        }
        let ty = typed(&s, 2);
        let ids: Vec<&str> = ty.iter().map(|t| t.unit_id.as_str()).collect();
        assert_eq!(ids, ["t:2:n", "t:2:tb:0:0", "t:2:tb:1:1", "t:2:low"]);
        assert_eq!(ty[3].at_ms, Some(761_000));
        assert_eq!(ty[3].text, "Budget  owner?");
        assert_eq!(
            (ty[1].at_ms, ty[2].at_ms),
            (Some(5), Some(9)),
            "a cell's own time, else the table's"
        );
        assert_eq!(ty[2].bbox, [160.0, 148.0, 320.0, 196.0]);
    }

    #[test]
    fn the_block_is_headed_and_capped() {
        assert_eq!(block(&[]), None);
        assert_eq!(
            block(&["a  b".into(), "c".into()]).unwrap(),
            "The user's typed notes:\n- a b\n- c"
        );
        let long = block(&["x".repeat(9_000)]).unwrap();
        assert_eq!(long.chars().count(), MAX_TYPED_CHARS);
    }
}
