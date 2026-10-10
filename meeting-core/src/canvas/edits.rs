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
    fn editable_item(&self, r: &Ref) -> Option<&Item> {
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
