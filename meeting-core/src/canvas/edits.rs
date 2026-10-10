// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/ShapeEdits.kt`,
// `ImageElements.kt` (`resize`), `TableEdits.kt` (`stretch`) and `CanvasActions.kt` (z-order,
// group, ungroup), MIT, Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
//! F21-FR5: what a drag on a selected thing's handle does (resize from a corner, rotate about
//! the centre, move a line's end), and arranging: front, back, group and ungroup. Each is one
//! step of the scene's history.
use super::geom::{self, Pt};
use super::item::{Item, MAX_ROW_HEIGHT, MIN_ROW_HEIGHT, Ref};
use super::scene::Scene;
use crate::ink::{BBox, StrokeId};

/// A box's corner, as a handle names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// `tl`, `tr`, `bl` or `br`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Corner> {
        match s {
            "tl" => Some(Corner::TopLeft),
            "tr" => Some(Corner::TopRight),
            "bl" => Some(Corner::BottomLeft),
            "br" => Some(Corner::BottomRight),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Corner::TopLeft => 0,
            Corner::TopRight => 1,
            Corner::BottomLeft => 2,
            Corner::BottomRight => 3,
        }
    }

    fn opposite(self) -> Corner {
        match self {
            Corner::TopLeft => Corner::BottomRight,
            Corner::TopRight => Corner::BottomLeft,
            Corner::BottomLeft => Corner::TopRight,
            Corner::BottomRight => Corner::TopLeft,
        }
    }
}

/// The narrowest a resize leaves an image (`ImageElements.MIN_WIDTH`).
pub const MIN_IMAGE_WIDTH: f64 = 8.0;

/// What the shell measured for a resize: a text box's or note's new text height, or a
/// table's cells' text heights at the new column width.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    pub fit: Option<f64>,
    pub cell_heights: Option<Vec<f64>>,
}

/// The box `it` takes when `corner` is dragged to `p`, the opposite corner fixed in world
/// space (`ShapeEdits.resizeCorner`, `resizeRotatedCorner`).
#[must_use]
pub fn resized_box(it: &Item, corner: Corner, p: Pt) -> (f64, f64, f64, f64) {
    let fixed = geom::corners(it)[corner.opposite().index()];
    let (s, c) = (-it.rotation).sin_cos();
    let (dx, dy) = (p.x - fixed.x, p.y - fixed.y);
    let (w, h) = ((dx * c - dy * s).abs(), (dx * s + dy * c).abs());
    let (cx, cy) = ((fixed.x + p.x) / 2.0, (fixed.y + p.y) / 2.0);
    (cx - w / 2.0, cy - h / 2.0, w, h)
}

/// An image's box when `corner` goes to `p`: its proportions kept, in its rotated frame
/// (`ImageElements.resize`).
#[must_use]
pub fn resized_image(it: &Item, corner: Corner, p: Pt, aspect: f64) -> (f64, f64, f64, f64) {
    let fixed = geom::corners(it)[corner.opposite().index()];
    let (sin, cos) = it.rotation.sin_cos();
    let (dx, dy) = (p.x - fixed.x, p.y - fixed.y);
    let along_x = dx * cos + dy * sin;
    let along_y = -dx * sin + dy * cos;
    let w = along_x
        .abs()
        .max(along_y.abs() * aspect)
        .max(MIN_IMAGE_WIDTH);
    let h = w / aspect;
    let (ddx, ddy) = (
        if along_x < 0.0 { -w } else { w },
        if along_y < 0.0 { -h } else { h },
    );
    let cx = fixed.x + (ddx * cos - ddy * sin) / 2.0;
    let cy = fixed.y + (ddx * sin + ddy * cos) / 2.0;
    (cx - w / 2.0, cy - h / 2.0, w, h)
}

impl Scene {
    pub(crate) fn editable_item(&self, r: &Ref) -> Option<&Item> {
        match r {
            Ref::Item(i) => self.item(i).filter(|it| it.known()),
            Ref::Stroke(_) => None,
        }
    }

    /// F21-FR5: resize one item or stroke by dragging `corner` to `p`, as one step. An image
    /// keeps its proportions; a stroke scales its points (pressure and times kept); a text box
    /// keeps its text size and takes the measured height, a note at least square; a table
    /// stretches its rows. Lines and arrows use their end handles instead.
    pub fn resize(&mut self, r: &Ref, corner: Corner, p: Pt, m: &Measured) -> Option<BBox> {
        if !p.x.is_finite() || !p.y.is_finite() || !self.items_ok() {
            return None;
        }
        if let Ref::Stroke(n) = r {
            let b = self.ink().get(StrokeId(*n))?.bounds();
            let mut it = Item::new("s", "rectangle", super::style::Style::default());
            (it.x, it.y, it.width, it.height) = (
                f64::from(b.x0),
                f64::from(b.y0),
                f64::from(b.x1 - b.x0),
                f64::from(b.y1 - b.y0),
            );
            let (x, y, w, h) = resized_box(&it, corner, p);
            let sx = if it.width > 0.0 { w / it.width } else { 1.0 };
            let sy = if it.height > 0.0 { h / it.height } else { 1.0 };
            return self.step_strokes(std::slice::from_ref(r), move |pt| {
                (x + (pt.x - it.x) * sx, y + (pt.y - it.y) * sy)
            });
        }
        let it = self.editable_item(r)?.clone();
        if it.has_ends() {
            return None;
        }
        let (x, y, w, h) = match &it.image {
            Some(img) => {
                resized_image(&it, corner, p, f64::from(img.width) / f64::from(img.height))
            }
            None => resized_box(&it, corner, p),
        };
        let fit = m.fit.filter(|f| f.is_finite() && *f > 0.0);
        self.edit_item(&it.id, |n| {
            (n.x, n.y, n.width, n.height) = (x, y, w, h);
            match n.kind.as_str() {
                "text" => n.height = fit.unwrap_or(n.height).min(super::item::MAX_COORD),
                "note" => n.height = fit.unwrap_or(0.0).max(n.width),
                "table" => stretch(n, m.cell_heights.as_deref()),
                _ => {}
            }
        })
    }

    /// F21-FR5: rotate one item (or stroke) so its rotate handle points at `p`, snapped near
    /// multiples of 45° (`ShapeEdits.rotateElement`), as one step. A stroke turns its points
    /// about its centre by that angle.
    pub fn rotate(&mut self, r: &Ref, p: Pt) -> Option<BBox> {
        if !p.x.is_finite() || !p.y.is_finite() || !self.items_ok() {
            return None;
        }
        if let Ref::Stroke(n) = r {
            let b = self.ink().get(StrokeId(*n))?.bounds();
            let c = Pt::new(f64::from(b.x0 + b.x1) / 2.0, f64::from(b.y0 + b.y1) / 2.0);
            let a = geom::snap_rotation((p.y - c.y).atan2(p.x - c.x) + std::f64::consts::FRAC_PI_2);
            let (s, co) = a.sin_cos();
            return self.step_strokes(std::slice::from_ref(r), move |pt| {
                let (dx, dy) = (pt.x - c.x, pt.y - c.y);
                (c.x + dx * co - dy * s, c.y + dx * s + dy * co)
            });
        }
        let it = self.editable_item(r)?.clone();
        if it.has_ends() {
            return None;
        }
        let c = geom::centre(&it);
        let a = geom::snap_rotation((p.y - c.y).atan2(p.x - c.x) + std::f64::consts::FRAC_PI_2);
        self.edit_item(&it.id, |n| n.rotation = a)
    }

    /// F21-FR5 / FR8: move a line's or arrow's end to `p`; released over a shape (any item but
    /// a line or arrow, or a stroke) it binds to it, elsewhere it is free (`moveEndpoint`).
    pub fn move_end(&mut self, r: &Ref, start: bool, p: Pt) -> Option<BBox> {
        if !p.x.is_finite() || !p.y.is_finite() || !self.items_ok() {
            return None;
        }
        let it = self.editable_item(r)?.clone();
        if !it.has_ends() {
            return None;
        }
        let bind = self.binding_target(p, Some(&it.id));
        self.edit_item(&it.id, |n| {
            let e = if start {
                n.start.as_mut()
            } else {
                n.end.as_mut()
            };
            if let Some(e) = e {
                (e.x, e.y, e.bind) = (p.x, p.y, bind);
            }
        })
    }

    /// The shape an arrow end at `p` binds to: the topmost item that isn't a line or arrow
    /// (nor `except`), or a stroke (`CanvasCore.bindingTargetAt`).
    #[must_use]
    pub fn binding_target(&self, p: Pt, except: Option<&str>) -> Option<Ref> {
        self.order().iter().rev().find_map(|r| match r {
            Ref::Item(i) => self
                .item(i)
                .filter(|it| {
                    !it.has_ends() && Some(it.id.as_str()) != except && geom::contains(it, p)
                })
                .map(|_| r.clone()),
            #[allow(clippy::cast_possible_truncation)]
            Ref::Stroke(n) => {
                let hit = self.ink().strokes_hit(p.x as f32, p.y as f32, 8.0);
                hit.contains(&StrokeId(*n)).then(|| r.clone())
            }
        })
    }

    /// F21-FR5: `refs` (and their groups) to the top or the bottom of the paint order, their
    /// own order kept (`CanvasActions.bringToFront`, `sendToBack`), as one step.
    pub fn arrange(&mut self, refs: &[Ref], front: bool) -> Option<BBox> {
        if !self.items_ok() {
            return None;
        }
        let moving = self.with_groups(refs);
        if moving.is_empty() {
            return None;
        }
        self.step_doc(|d| {
            let rest: Vec<Ref> = d
                .order
                .iter()
                .filter(|r| !moving.contains(r))
                .cloned()
                .collect();
            d.order = if front {
                rest.into_iter().chain(moving.iter().cloned()).collect()
            } else {
                moving.iter().cloned().chain(rest).collect()
            };
        })
    }

    /// F21-FR5: `refs` and the rest of their groups made one group (groups don't nest), as one
    /// step; `None` for fewer than two things. The new group's id.
    pub fn group(&mut self, refs: &[Ref]) -> Option<(String, BBox)> {
        if !self.items_ok() {
            return None;
        }
        let members = self.with_groups(refs);
        if members.len() < 2 {
            return None;
        }
        let g = format!("g-{}", self.fresh_id());
        let area = self.set_group(&members, Some(&g))?;
        self.selection = members;
        Some((g, area))
    }

    /// F21-FR5: `refs` and the rest of their groups left on their own again, as one step.
    pub fn ungroup(&mut self, refs: &[Ref]) -> Option<BBox> {
        if !self.items_ok() {
            return None;
        }
        let members = self.with_groups(refs);
        self.set_group(&members, None)
    }

    fn set_group(&mut self, members: &[Ref], g: Option<&str>) -> Option<BBox> {
        let g = g.map(str::to_owned);
        self.step_doc(|d| {
            for r in members {
                match r {
                    Ref::Item(i) => {
                        if let Some(it) = d.items.iter_mut().find(|x| x.id == *i) {
                            it.group_id.clone_from(&g);
                        }
                    }
                    Ref::Stroke(n) => {
                        let e = d.extras.entry(*n).or_default();
                        e.group_id.clone_from(&g);
                        if e.is_empty() {
                            d.extras.remove(n);
                        }
                    }
                }
            }
        })
    }
}

impl Scene {
    /// F21-FR6: one style pick on `refs` (their groups included), as one step: each takes it
    /// where it applies (`style::applies`). A pen stroke's colour, opacity and size also set
    /// its true RGBA and width in `.inkbin`; its style ids go in `items.json`.
    pub fn restyle(&mut self, refs: &[Ref], property: &str, value: &str) -> Option<BBox> {
        if !self.items_ok() {
            return None;
        }
        let refs = self.with_groups(refs);
        let ink_refs: Vec<Ref> = refs
            .iter()
            .filter(|r| r.stroke().is_some() && super::style::applies("stroke", property, value))
            .cloned()
            .collect();
        let mut styles = Vec::new();
        for r in &ink_refs {
            if let Ref::Stroke(n) = r {
                let base = self
                    .doc()
                    .extras
                    .get(n)
                    .and_then(|e| e.style.clone())
                    .unwrap_or_default();
                styles.push((*n, base.with(property, value)));
            }
        }
        self.step(|s| {
            let st = styles.clone();
            let (removed, added) = s.map_strokes(&ink_refs, |stroke| {
                if stroke.tool != crate::ink::Tool::Pen {
                    return;
                }
                if let Some((_, style)) = st.iter().find(|(n, _)| *n == stroke.id.0) {
                    stroke.color = ink_color(style);
                    stroke.width = style.stroke_width();
                }
            });
            for (n, style) in styles {
                s.doc_mut().extras.entry(n).or_default().style = Some(style);
            }
            for it in &mut s.doc_mut().items {
                if refs.contains(&Ref::Item(it.id.clone()))
                    && it.known()
                    && super::style::applies(&it.kind, property, value)
                {
                    it.style = it.style.with(property, value);
                }
            }
            (removed, added)
        })
    }
}

/// What a creation needs besides its geometry: the style it takes (the default), when it was
/// made (`recMs` on the recording clock, or untimed) and the local time.
#[derive(Clone, Debug)]
pub struct Made<'a> {
    pub style: super::style::Style,
    pub rec_ms: Option<u64>,
    pub created_at: &'a str,
}

impl Scene {
    /// Add `it` on top as one step (its id made unique here). Its ref.
    pub fn add_item(&mut self, mut it: Item) -> Option<Ref> {
        if !self.items_ok() {
            return None;
        }
        if it.id.is_empty() || self.item(&it.id).is_some() {
            it.id = self.fresh_id();
        }
        let r = Ref::Item(it.id.clone());
        let rr = r.clone();
        self.step_doc(move |d| {
            d.order.push(rr);
            d.items.push(it);
        })?;
        Some(r)
    }

    /// F21-FR8: a rectangle or ellipse from a pen drag, corner to corner (`boxFromDrag`), or a
    /// line or arrow from start to end whose ends bind to the shapes they were drawn from and
    /// to (`connectorFromDrag`, `bindingTargetAt`). In the default style; one step.
    pub fn create_shape(&mut self, kind: &str, from: Pt, to: Pt, made: &Made<'_>) -> Option<Ref> {
        let finite = [from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite());
        if !finite || !matches!(kind, "rectangle" | "ellipse" | "line" | "arrow") {
            return None;
        }
        let mut it = Item::new("", kind, made.style.clone());
        it.rec_ms = made.rec_ms;
        it.created_at = Some(made.created_at.to_owned());
        if matches!(kind, "line" | "arrow") {
            it.start = Some(super::item::End {
                x: from.x,
                y: from.y,
                bind: self.binding_target(from, None),
            });
            it.end = Some(super::item::End {
                x: to.x,
                y: to.y,
                bind: self.binding_target(to, None),
            });
        } else {
            (it.x, it.y) = (from.x.min(to.x), from.y.min(to.y));
            (it.width, it.height) = ((to.x - from.x).abs(), (to.y - from.y).abs());
        }
        self.add_item(it)
    }
}

/// A new text box's width in world units; its height follows its text (`TextElements.kt`).
pub const TEXT_WIDTH: f64 = 320.0;
/// A new sticky note's side; it stays at least square.
pub const NOTE_SIZE: f64 = 200.0;
/// The least a text box is tall (one line's padding).
pub const MIN_TEXT_HEIGHT: f64 = 16.0;

/// A measured text height, clamped (the shell measures; the core keeps it sane).
fn text_height(h: Option<f64>) -> f64 {
    h.filter(|h| h.is_finite())
        .unwrap_or(MIN_TEXT_HEIGHT)
        .clamp(MIN_TEXT_HEIGHT, super::item::MAX_COORD)
}

impl Scene {
    /// F21-FR9: a text box (its top-left at `at`, 320 wide) or a sticky note (centred on
    /// `at`, at least square), with its first text, as one step; an empty one is not made. Its
    /// `recMs` is when that text was committed (OQ36).
    pub fn create_text(
        &mut self,
        kind: &str,
        at: Pt,
        text: &str,
        height: Option<f64>,
        made: &Made<'_>,
    ) -> Option<Ref> {
        if !matches!(kind, "text" | "note")
            || text.trim().is_empty()
            || text.len() > super::item::MAX_TEXT
        {
            return None;
        }
        if !at.x.is_finite() || !at.y.is_finite() {
            return None;
        }
        let mut it = Item::new("", kind, made.style.clone());
        it.text = Some(text.to_owned());
        it.rec_ms = made.rec_ms;
        it.created_at = Some(made.created_at.to_owned());
        let h = text_height(height);
        if kind == "text" {
            (it.x, it.y, it.width, it.height) = (at.x, at.y, TEXT_WIDTH, h);
        } else {
            let side = NOTE_SIZE;
            (it.x, it.y, it.width, it.height) =
                (at.x - side / 2.0, at.y - side / 2.0, side, h.max(side));
        }
        self.add_item(it)
    }

    /// F21-FR9: a text box's or note's text set (one step per editing session), its height
    /// refitted. A text box left blank is removed, as in tldraw; a note keeps its square. A
    /// first text written while recording times it, if it had no time (OQ36).
    pub fn set_text(
        &mut self,
        r: &Ref,
        text: &str,
        height: Option<f64>,
        rec_ms: Option<u64>,
    ) -> Option<BBox> {
        let it = self.editable_item(r)?.clone();
        if !matches!(it.kind.as_str(), "text" | "note") || text.len() > super::item::MAX_TEXT {
            return None;
        }
        if it.kind == "text" && text.trim().is_empty() {
            return self.delete(std::slice::from_ref(r)).1;
        }
        let first = it.text.as_deref().unwrap_or("").is_empty();
        self.edit_item(&it.id, |n| {
            n.text = Some(text.to_owned());
            let h = text_height(height.or(Some(n.height)));
            n.height = if n.kind == "note" { h.max(n.width) } else { h };
            if first && n.rec_ms.is_none() {
                n.rec_ms = rec_ms;
            }
        })
    }
}

impl Scene {
    /// F21-FR13: link `refs` (and their groups), items and strokes alike, to `link` (or to
    /// nothing), as one step.
    pub fn relink(&mut self, refs: &[Ref], link: Option<&super::item::Link>) -> Option<BBox> {
        if !self.items_ok() {
            return None;
        }
        let refs = self.with_groups(refs);
        self.step_doc(|d| {
            for r in &refs {
                match r {
                    Ref::Item(i) => {
                        if let Some(it) = d.items.iter_mut().find(|x| x.id == *i && x.known()) {
                            it.link = link.cloned();
                        }
                    }
                    Ref::Stroke(n) => {
                        let e = d.extras.entry(*n).or_default();
                        e.link = link.cloned();
                        if e.is_empty() {
                            d.extras.remove(n);
                        }
                    }
                }
            }
        })
    }

    /// The link of `r`, if it has one.
    #[must_use]
    pub fn link_of(&self, r: &Ref) -> Option<&super::item::Link> {
        match r {
            Ref::Item(i) => self.item(i)?.link.as_ref(),
            Ref::Stroke(n) => self.doc().extras.get(n)?.link.as_ref(),
        }
    }
}

/// A style's true colour as `.inkbin` keeps it (F21-FR7): its RGB, its opacity as alpha.
#[must_use]
pub fn ink_color(style: &super::style::Style) -> crate::ink::InkColor {
    let rgb = style.rgb();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // opacity is 0.1..=1
    let a = (style.opacity * 255.0).round() as u8;
    crate::ink::InkColor::rgba((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, a)
}

/// A table resized to a new height: every row stretched in proportion (`TableEdits.stretch`),
/// within the row limits, its cells' text heights as measured at the new width.
fn stretch(n: &mut Item, cells: Option<&[f64]>) {
    let height = n.height;
    let Some(t) = n.table.as_mut() else { return };
    if let Some(c) = cells.filter(|c| c.len() == t.cells.len()) {
        t.cell_heights = c.iter().map(|h| h.clamp(0.0, MAX_ROW_HEIGHT)).collect();
    }
    let rows = t.row_heights();
    let total: f64 = rows.iter().sum();
    let k = if total > 0.0 { height / total } else { 1.0 };
    t.row_min_heights = rows
        .iter()
        .map(|h| (h * k).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT))
        .collect();
    n.height = t.row_heights().iter().sum();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::hit::outline_distance;
    use crate::canvas::item::{End, Image, Table};
    use crate::canvas::scene::tests::{pen, rect, with_item};
    use crate::canvas::style::Style;
    use crate::ink::InkLayer;
    use std::f64::consts::PI;

    fn empty() -> Scene {
        Scene::load(InkLayer::new(), Ok(None)).0
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn arrow(s: &mut Scene) {
        let mut ar = Item::new("ar", "arrow", Style::default());
        ar.start = Some(End {
            x: 0.0,
            y: 0.0,
            bind: Some(Ref::Item("a".into())),
        });
        ar.end = Some(End {
            x: 0.0,
            y: 0.0,
            bind: Some(Ref::Item("b".into())),
        });
        with_item(s, ar);
    }

    /// F21-AC8: an arrow bound to A and B: A moved, turned 30° or resized, the resolved start
    /// lies on A's boundary, facing B, with no edit of the arrow's own.
    #[test]
    fn a_bound_arrow_follows_its_shape_through_every_edit() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        with_item(&mut s, rect("b", 600.0, 0.0));
        arrow(&mut s);
        let stored = s.item("ar").unwrap().clone();
        let a = Ref::Item("a".into());
        let check = |s: &Scene| {
            let (st, en) = s.ends(s.item("ar").unwrap()).unwrap();
            let shape = s.item("a").unwrap();
            assert!(
                outline_distance(shape, st).abs() < 1e-6,
                "on A's edge: {st:?}"
            );
            let c = geom::centre(shape);
            let toward = ((st.x - c.x) * (en.x - c.x) + (st.y - c.y) * (en.y - c.y)) > 0.0;
            assert!(toward, "facing B");
            assert_eq!(s.item("ar").unwrap(), &stored, "no edit of its own");
        };
        s.move_refs(std::slice::from_ref(&a), 200.0, 0.0);
        check(&s);
        let c = geom::centre(s.item("a").unwrap());
        s.rotate(&a, Pt::new(c.x + 100.0, c.y - 100.0 * (PI / 3.0).tan()));
        assert!(
            close(s.item("a").unwrap().rotation, PI / 6.0),
            "{}",
            s.item("a").unwrap().rotation
        );
        check(&s);
        s.resize(
            &a,
            Corner::BottomRight,
            Pt::new(400.0, 300.0),
            &Measured::default(),
        );
        check(&s);
    }

    #[test]
    fn corners_resize_and_rotation_snaps() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        let a = Ref::Item("a".into());
        s.resize(
            &a,
            Corner::TopLeft,
            Pt::new(-50.0, -50.0),
            &Measured::default(),
        );
        let it = s.item("a").unwrap();
        assert_eq!(
            (it.x, it.y, it.width, it.height),
            (-50.0, -50.0, 150.0, 100.0)
        );
        let c = geom::centre(it);
        s.rotate(&a, Pt::new(c.x + 100.0, c.y - 97.0)); // 46° from up: snaps to 45°
        assert!(close(s.item("a").unwrap().rotation, PI / 4.0));
        // A rotated box keeps the opposite corner fixed in world space.
        let fixed = geom::corners(s.item("a").unwrap())[0];
        s.resize(
            &a,
            Corner::BottomRight,
            Pt::new(fixed.x, fixed.y + 200.0),
            &Measured::default(),
        );
        let after = geom::corners(s.item("a").unwrap())[0];
        assert!(close(after.x, fixed.x) && close(after.y, fixed.y));
        for c in ["tl", "tr", "bl", "br"] {
            assert!(Corner::parse(c).is_some());
        }
        assert_eq!(Corner::parse("x"), None);
        assert_eq!(
            s.resize(
                &a,
                Corner::TopLeft,
                Pt::new(f64::NAN, 0.0),
                &Measured::default()
            ),
            None
        );
        assert_eq!(s.rotate(&Ref::Item("zz".into()), Pt::new(0.0, 0.0)), None);
    }

    /// F21-AC11 (part): an image resized from a corner keeps its aspect within 0.5 %.
    #[test]
    fn images_keep_their_proportions() {
        let mut s = empty();
        let mut im = rect("im", 0.0, 0.0);
        im.kind = "image".into();
        (im.width, im.height) = (160.0, 120.0);
        im.image = Some(Image {
            file: "im.jpg".into(),
            width: 1600,
            height: 1200,
        });
        im.rotation = 0.3;
        with_item(&mut s, im);
        s.resize(
            &Ref::Item("im".into()),
            Corner::BottomRight,
            Pt::new(500.0, 90.0),
            &Measured::default(),
        );
        let it = s.item("im").unwrap();
        assert!((it.width / it.height - 4.0 / 3.0).abs() / (4.0 / 3.0) < 0.005);
        s.resize(
            &Ref::Item("im".into()),
            Corner::TopLeft,
            Pt::new(500.0, 500.0),
            &Measured::default(),
        );
        assert!(s.item("im").unwrap().width >= MIN_IMAGE_WIDTH);
    }

    #[test]
    fn text_notes_and_tables_refit() {
        let mut s = empty();
        let mut t = rect("t", 0.0, 0.0);
        t.kind = "text".into();
        let mut n = rect("n", 0.0, 500.0);
        n.kind = "note".into();
        let mut tb = rect("tb", 0.0, 900.0);
        tb.kind = "table".into();
        tb.table = Some(Table::empty(2, 2));
        (tb.width, tb.height) = (320.0, 96.0);
        with_item(&mut s, t);
        with_item(&mut s, n);
        with_item(&mut s, tb);
        let fit = Measured {
            fit: Some(77.0),
            cell_heights: None,
        };
        s.resize(
            &Ref::Item("t".into()),
            Corner::BottomRight,
            Pt::new(200.0, 10.0),
            &fit,
        );
        assert_eq!(
            (s.item("t").unwrap().width, s.item("t").unwrap().height),
            (200.0, 77.0)
        );
        s.resize(
            &Ref::Item("n".into()),
            Corner::BottomRight,
            Pt::new(300.0, 510.0),
            &fit,
        );
        assert_eq!(
            s.item("n").unwrap().height,
            300.0,
            "a note stays at least square"
        );
        let cells = Measured {
            fit: None,
            cell_heights: Some(vec![150.0, 0.0, 0.0, 0.0]),
        };
        s.resize(
            &Ref::Item("tb".into()),
            Corner::BottomRight,
            Pt::new(640.0, 1092.0),
            &cells,
        );
        let t = s.item("tb").unwrap().table.clone().unwrap();
        assert_eq!(t.row_heights()[0], 150.0, "never below its text");
        assert!(t.row_heights()[1] >= 48.0);
        assert_eq!(
            s.item("tb").unwrap().height,
            t.row_heights().iter().sum::<f64>()
        );
    }

    #[test]
    fn strokes_scale_and_turn_keeping_their_times() {
        let mut s = empty();
        let st = pen(&mut s, &[(0.0, 0.0), (100.0, 50.0)], Some(42));
        let r = Ref::Stroke(st.0);
        s.resize(
            &r,
            Corner::BottomRight,
            Pt::new(201.0, 101.0),
            &Measured::default(),
        );
        let p = s.ink().get(st).unwrap().points.clone();
        assert!(
            (p[1].x - 199.0).abs() < 0.05 && (p[1].y - 99.0).abs() < 0.05,
            "{p:?}"
        );
        assert_eq!(s.ink().get(st).unwrap().rec_ms, Some(42));
        assert_eq!(p[0].pressure, 0.5);
        let b = s.ink().get(st).unwrap().bounds();
        let c = Pt::new(f64::from(b.x0 + b.x1) / 2.0, f64::from(b.y0 + b.y1) / 2.0);
        s.rotate(&r, Pt::new(c.x + 500.0, c.y)); // 90°
        let q = &s.ink().get(st).unwrap().points;
        assert!((f64::from(q[0].x) - (c.x + (c.y - f64::from(p[0].y)))).abs() < 0.01);
        assert_eq!(
            s.resize(
                &Ref::Stroke(99),
                Corner::TopLeft,
                Pt::new(0.0, 0.0),
                &Measured::default()
            ),
            None
        );
    }

    #[test]
    fn line_ends_bind_where_released() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        with_item(&mut s, rect("b", 600.0, 0.0));
        arrow(&mut s);
        let ar = Ref::Item("ar".into());
        s.move_end(&ar, false, Pt::new(300.0, 300.0));
        assert_eq!(
            s.item("ar").unwrap().end.as_ref().unwrap().bind,
            None,
            "dragged away: free"
        );
        s.move_end(&ar, true, Pt::new(650.0, 25.0));
        assert_eq!(
            s.item("ar").unwrap().start.as_ref().unwrap().bind,
            Some(Ref::Item("b".into()))
        );
        let st = pen(&mut s, &[(0.0, 900.0), (100.0, 900.0)], None);
        s.move_end(&ar, false, Pt::new(50.0, 902.0));
        assert_eq!(
            s.item("ar").unwrap().end.as_ref().unwrap().bind,
            Some(Ref::Stroke(st.0))
        );
        assert_eq!(
            s.move_end(&Ref::Item("a".into()), true, Pt::new(0.0, 0.0)),
            None
        );
        assert_eq!(
            s.resize(
                &ar,
                Corner::TopLeft,
                Pt::new(0.0, 0.0),
                &Measured::default()
            ),
            None
        );
        assert_eq!(s.rotate(&ar, Pt::new(0.0, 0.0)), None);
        assert_eq!(s.move_end(&ar, true, Pt::new(f64::NAN, 0.0)), None);
    }

    /// F21-AC7 (grouping part): Group, then a tap on one member selects all three; one move
    /// moves them; Ungroup leaves three again. Front and back keep their own order.
    #[test]
    fn groups_select_move_and_arrange_as_one() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        with_item(&mut s, rect("b", 200.0, 0.0));
        let st = pen(&mut s, &[(0.0, 300.0), (50.0, 300.0)], None);
        with_item(&mut s, rect("c", 900.0, 900.0));
        let three = vec![
            Ref::Item("a".into()),
            Ref::Item("b".into()),
            Ref::Stroke(st.0),
        ];
        assert_eq!(s.group(&three[..1]), None, "one thing is no group");
        let (g, _) = s.group(&three).unwrap();
        assert!(g.starts_with("g-"));
        let tap = s.top_at(Pt::new(250.0, 25.0), 4.0).unwrap();
        s.select(&[tap]);
        assert_eq!(s.selection, three);
        let sel = s.selection.clone();
        s.move_refs(&sel, 10.0, 0.0);
        assert_eq!(
            (s.item("a").unwrap().x, s.item("b").unwrap().x),
            (10.0, 210.0)
        );
        s.arrange(&[Ref::Item("a".into())], false);
        let order: Vec<String> = s.order().iter().map(ToString::to_string).collect();
        assert_eq!(
            order,
            ["i:a", "i:b", "s:0", "i:c"],
            "the whole group, in its order"
        );
        s.arrange(&[Ref::Item("a".into())], true);
        assert_eq!(s.order().last(), Some(&Ref::Stroke(st.0)));
        s.ungroup(&[Ref::Item("b".into())]);
        assert!(s.group_of(&Ref::Item("a".into())).is_none() && s.doc().extras.is_empty());
        s.undo();
        assert_eq!(s.group_of(&Ref::Stroke(st.0)), Some(g.as_str()));
        assert_eq!(s.arrange(&[], true), None);
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        assert_eq!(broken.group(&three), None);
        assert_eq!(broken.ungroup(&three), None);
        assert_eq!(broken.arrange(&three, true), None);
        assert_eq!(
            broken.resize(
                &three[0],
                Corner::TopLeft,
                Pt::new(0.0, 0.0),
                &Measured::default()
            ),
            None
        );
        assert_eq!(broken.rotate(&three[0], Pt::new(0.0, 0.0)), None);
        assert_eq!(broken.move_end(&three[0], true, Pt::new(0.0, 0.0)), None);
    }

    /// F21-AC9 (host part): a shape and a stroke restyled blue and L in one step each; an
    /// option that doesn't apply leaves a thing alone.
    #[test]
    fn a_pick_restyles_the_selection_where_it_applies() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        let st = pen(&mut s, &[(0.0, 300.0), (50.0, 300.0)], Some(3));
        let both = [Ref::Item("a".into()), Ref::Stroke(st.0)];
        let steps = |s: &mut Scene| {
            let mut n = 0;
            while s.can_undo() {
                s.undo();
                n += 1;
            }
            for _ in 0..n {
                s.redo();
            }
            n
        };
        let before = steps(&mut s);
        assert!(s.restyle(&both, "color", "blue").is_some());
        assert!(s.restyle(&both, "size", "l").is_some());
        assert_eq!(steps(&mut s), before + 2, "one step per pick");
        assert_eq!(
            (
                s.item("a").unwrap().style.color,
                s.item("a").unwrap().style.size
            ),
            ("blue", "l")
        );
        let ink = s.ink().get(st).unwrap();
        assert_eq!(
            (ink.color, ink.width),
            (crate::ink::InkColor::rgba(0x44, 0x65, 0xE9, 255), 5.0)
        );
        assert_eq!(ink.rec_ms, Some(3));
        let es = s.doc().extras[&st.0].style.clone().unwrap();
        assert_eq!((es.color, es.size), ("blue", "l"));
        assert!(s.restyle(&both, "fill", "solid").is_some());
        assert_eq!(s.item("a").unwrap().style.fill, "solid");
        assert_eq!(
            s.doc().extras[&st.0].style.as_ref().unwrap().fill,
            "none",
            "no fill on ink"
        );
        assert_eq!(
            s.restyle(&both, "dash", "none"),
            None,
            "nothing it applies to"
        );
        assert_eq!(
            s.restyle(&both, "color", "magenta"),
            None,
            "unknown values change nothing"
        );
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        assert_eq!(broken.restyle(&both, "color", "red"), None);
        let half = crate::canvas::style::Style::default().with("opacity", "0.5");
        assert_eq!(ink_color(&half).a, 128);
    }

    /// F21-FR8: shapes by a drag; an arrow's ends bind to what they were drawn from and to.
    #[test]
    fn shapes_and_attached_arrows_are_drawn() {
        let mut s = empty();
        let made = Made {
            style: Style::default().with("color", "blue"),
            rec_ms: Some(9),
            created_at: "t",
        };
        let r = s
            .create_shape("rectangle", Pt::new(100.0, 80.0), Pt::new(0.0, 0.0), &made)
            .unwrap();
        let Ref::Item(id) = &r else { panic!() };
        let it = s.item(id).unwrap().clone();
        assert_eq!((it.x, it.y, it.width, it.height), (0.0, 0.0, 100.0, 80.0));
        assert_eq!((it.style.color, it.rec_ms), ("blue", Some(9)));
        let e = s
            .create_shape("ellipse", Pt::new(400.0, 0.0), Pt::new(500.0, 80.0), &made)
            .unwrap();
        let ar = s
            .create_shape("arrow", Pt::new(50.0, 40.0), Pt::new(450.0, 40.0), &made)
            .unwrap();
        let Ref::Item(aid) = &ar else { panic!() };
        let a = s.item(aid).unwrap();
        assert_eq!(a.start.as_ref().unwrap().bind, Some(r.clone()));
        assert_eq!(a.end.as_ref().unwrap().bind, Some(e));
        let free = s
            .create_shape("line", Pt::new(900.0, 900.0), Pt::new(950.0, 900.0), &made)
            .unwrap();
        let Ref::Item(fid) = &free else { panic!() };
        assert_eq!(s.item(fid).unwrap().start.as_ref().unwrap().bind, None);
        assert_eq!(s.order().len(), 4);
        s.undo();
        assert_eq!(s.order().len(), 3);
        assert_eq!(
            s.create_shape("hexagon", Pt::new(0.0, 0.0), Pt::new(1.0, 1.0), &made),
            None
        );
        assert_eq!(
            s.create_shape("line", Pt::new(f64::NAN, 0.0), Pt::new(1.0, 1.0), &made),
            None
        );
        let mut dup = rect("a", 0.0, 0.0);
        dup.id = id.clone();
        assert_ne!(s.add_item(dup), Some(r), "a taken id gets a fresh one");
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        assert_eq!(broken.add_item(rect("b", 0.0, 0.0)), None);
    }

    /// F21-FR9: text boxes and notes, made with their first text, timed then; edits refit
    /// them and never re-time them; a blank text box goes.
    #[test]
    fn text_boxes_and_sticky_notes() {
        let mut s = empty();
        let made = Made {
            style: Style::default(),
            rec_ms: Some(761_000),
            created_at: "t",
        };
        assert_eq!(
            s.create_text("text", Pt::new(0.0, 0.0), "  ", Some(30.0), &made),
            None,
            "empty: discarded"
        );
        let t = s
            .create_text(
                "text",
                Pt::new(10.0, 20.0),
                "Budget owner?",
                Some(40.0),
                &made,
            )
            .unwrap();
        let Ref::Item(tid) = &t else { panic!() };
        let it = s.item(tid).unwrap().clone();
        assert_eq!(
            (it.x, it.y, it.width, it.height, it.rec_ms),
            (10.0, 20.0, 320.0, 40.0, Some(761_000))
        );
        let n = s
            .create_text("note", Pt::new(500.0, 500.0), "Ask Sam", Some(30.0), &made)
            .unwrap();
        let Ref::Item(nid) = &n else { panic!() };
        let note = s.item(nid).unwrap().clone();
        assert_eq!((note.x, note.width, note.height), (400.0, 200.0, 200.0));
        assert!(
            s.set_text(&t, "Budget owner? Sam", Some(80.0), Some(999))
                .is_some()
        );
        let it = s.item(tid).unwrap();
        assert_eq!(
            (it.height, it.rec_ms),
            (80.0, Some(761_000)),
            "edits never re-time it"
        );
        assert!(s.set_text(&n, "", Some(10.0), None).is_some());
        assert_eq!(
            s.item(nid).unwrap().height,
            200.0,
            "a blank note keeps its square"
        );
        assert!(s.set_text(&n, "now timed", Some(10.0), Some(5)).is_some());
        assert_eq!(
            s.item(nid).unwrap().rec_ms,
            Some(761_000),
            "it already had a time"
        );
        s.set_text(&t, "   ", None, None);
        assert!(s.item(tid).is_none(), "a blank text box goes");
        assert_eq!(
            s.create_text("rect", Pt::new(0.0, 0.0), "x", None, &made),
            None
        );
        assert_eq!(
            s.create_text("text", Pt::new(f64::NAN, 0.0), "x", None, &made),
            None
        );
        let r = s
            .create_shape("rectangle", Pt::new(0.0, 0.0), Pt::new(9.0, 9.0), &made)
            .unwrap();
        assert_eq!(s.set_text(&r, "x", None, None), None, "not a text item");
        let mut untimed = Item::new("u", "note", Style::default());
        untimed.text = Some(String::new());
        with_item(&mut s, untimed);
        s.set_text(&Ref::Item("u".into()), "first", Some(20.0), Some(42));
        assert_eq!(
            s.item("u").unwrap().rec_ms,
            Some(42),
            "its first text times it"
        );
    }

    /// F21-FR13: a link goes on every selected item and stroke in one step; Remove link clears it.
    #[test]
    fn links_go_on_items_and_strokes() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        let st = pen(&mut s, &[(0.0, 300.0)], None);
        let both = [Ref::Item("a".into()), Ref::Stroke(st.0)];
        let l = crate::canvas::item::Link::meeting("meeting-20261008-090000").unwrap();
        assert!(s.relink(&both, Some(&l)).is_some());
        assert_eq!(s.link_of(&both[0]), Some(&l));
        assert_eq!(s.link_of(&both[1]), Some(&l));
        assert!(s.relink(&both, None).is_some());
        assert_eq!(s.link_of(&both[1]), None);
        assert!(s.doc().extras.is_empty());
        assert_eq!(s.link_of(&Ref::Item("zz".into())), None);
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        assert_eq!(broken.relink(&both, Some(&l)), None);
    }

    #[test]
    fn unknown_types_are_not_edited() {
        let mut s = empty();
        let mut u = rect("u", 0.0, 0.0);
        u.kind = "sparkle".into();
        with_item(&mut s, u);
        let r = Ref::Item("u".into());
        assert_eq!(
            s.resize(
                &r,
                Corner::TopLeft,
                Pt::new(-9.0, -9.0),
                &Measured::default()
            ),
            None
        );
        assert_eq!(s.rotate(&r, Pt::new(0.0, 0.0)), None);
        assert!(s.move_refs(&[r], 5.0, 5.0).is_some(), "but it moves");
    }
}
