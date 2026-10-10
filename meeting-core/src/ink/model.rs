//! Ink types and the world-space [`InkLayer`] (adapted from `inkread-ink/src/model.rs`).
use std::fmt;

/// The ink domain's errors; the domain never panics on bad input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InkError {
    /// A coordinate, pressure or width was NaN or infinite (or a width not positive).
    NonFinite(&'static str),
    /// A stroke with no points.
    EmptyStroke,
    /// A `.inkbin` blob was malformed or truncated (see [`crate::ink::codec`]).
    BadEncoding(String),
}

impl fmt::Display for InkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InkError::NonFinite(w) => write!(f, "non-finite {w}"),
            InkError::EmptyStroke => write!(f, "stroke has no points"),
            InkError::BadEncoding(m) => write!(f, "malformed ink data: {m}"),
        }
    }
}

impl std::error::Error for InkError {}

/// The ink domain's result.
pub type InkResult<T> = Result<T, InkError>;

/// A drawing tool. The eraser removes strokes; it never deposits ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Highlighter,
    Eraser,
}

impl Tool {
    /// The wire code (`0` Pen, `1` Highlighter, `2` Eraser); unknown is `None`.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Tool> {
        match code {
            0 => Some(Tool::Pen),
            1 => Some(Tool::Highlighter),
            2 => Some(Tool::Eraser),
            _ => None,
        }
    }

    /// The stable wire code (inverse of [`Self::from_code`]).
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Tool::Pen => 0,
            Tool::Highlighter => 1,
            Tool::Eraser => 2,
        }
    }

    /// Whether it deposits ink.
    #[must_use]
    pub fn is_ink(self) -> bool {
        matches!(self, Tool::Pen | Tool::Highlighter)
    }
}

/// An RGBA colour, kept even on a grayscale panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl InkColor {
    /// Opaque black, the pen's colour.
    pub const BLACK: InkColor = InkColor {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// Build an RGBA colour.
    #[must_use]
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for InkColor {
    fn default() -> Self {
        InkColor::BLACK
    }
}

/// One sample along a stroke, in **world** coordinates (unbounded; R20). Pressure is `[0,1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InkPoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt_x: Option<f32>,
    pub tilt_y: Option<f32>,
    /// Milliseconds after the stroke's first point.
    pub timestamp_ms: u32,
}

impl InkPoint {
    /// A validated point: a non-finite x, y or pressure is refused; pressure is clamped to
    /// `[0,1]`; a non-finite tilt is dropped. Unlike inkread's page points, x and y are kept.
    pub fn new(
        x: f32,
        y: f32,
        pressure: f32,
        tilt_x: Option<f32>,
        tilt_y: Option<f32>,
        timestamp_ms: u32,
    ) -> InkResult<Self> {
        if !x.is_finite() || !y.is_finite() || !pressure.is_finite() {
            return Err(InkError::NonFinite("point coordinate"));
        }
        let finite = |t: Option<f32>| t.filter(|v| v.is_finite());
        Ok(Self {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            tilt_x: finite(tilt_x),
            tilt_y: finite(tilt_y),
            timestamp_ms,
        })
    }
}

/// A stroke's id, unique and increasing within a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StrokeId(pub u32);

/// A committed ink stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub id: StrokeId,
    /// Always an ink tool (Pen or Highlighter).
    pub tool: Tool,
    pub color: InkColor,
    /// Nominal width in world units.
    pub width: f32,
    /// At least one point.
    pub points: Vec<InkPoint>,
    /// When it was written, ms since the epoch (caller-supplied; F19-FR7's "new since").
    pub created_at_ms: u64,
    /// Its first point's recording-clock time (F17-FR4); `None` is untimed.
    pub rec_ms: Option<u64>,
}

impl Stroke {
    /// The stroke's bounds, grown by half its width.
    #[must_use]
    pub fn bounds(&self) -> BBox {
        let first = self.points[0];
        let mut b = BBox {
            x0: first.x,
            y0: first.y,
            x1: first.x,
            y1: first.y,
        };
        for p in &self.points[1..] {
            b.x0 = b.x0.min(p.x);
            b.y0 = b.y0.min(p.y);
            b.x1 = b.x1.max(p.x);
            b.y1 = b.y1.max(p.y);
        }
        let m = self.width * 0.5;
        BBox {
            x0: b.x0 - m,
            y0: b.y0 - m,
            x1: b.x1 + m,
            y1: b.y1 + m,
        }
    }
}

/// A world-space box: a stroke's extent, or a dirty rectangle to redraw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl BBox {
    /// The smallest box holding both.
    #[must_use]
    pub fn union(self, o: BBox) -> BBox {
        BBox {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    /// Whether the two share any point (edges included).
    #[must_use]
    pub fn meets(self, o: BBox) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }
}

/// A reversible edit: the unit of undo and redo (F13-FR5).
#[derive(Debug, Clone, PartialEq)]
enum Edit {
    /// A stroke was added at the end.
    Add(Stroke),
    /// Strokes were removed by one eraser sweep or lasso delete; `(index, stroke)` ascending.
    Remove(Vec<(usize, Stroke)>),
    /// A lasso selection was moved by `(dx, dy)`.
    Move {
        ids: Vec<StrokeId>,
        dx: f32,
        dy: f32,
    },
}

/// All the ink of one canvas, in paint order, with its session undo history (not saved).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InkLayer {
    strokes: Vec<Stroke>,
    next_id: u32,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl InkLayer {
    /// An empty layer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A layer of stored strokes (the codec's path); new ids continue past the largest.
    #[must_use]
    pub fn from_strokes(strokes: Vec<Stroke>) -> Self {
        let next_id = strokes
            .iter()
            .map(|s| s.id.0)
            .max()
            .map_or(0, |m| m.saturating_add(1));
        Self {
            strokes,
            next_id,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The strokes in paint order.
    #[must_use]
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// Whether it holds no stroke.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    /// The stroke `id`, if present.
    #[must_use]
    pub fn get(&self, id: StrokeId) -> Option<&Stroke> {
        self.strokes.iter().find(|s| s.id == id)
    }

    /// Commit one whole stroke (the batched path, F17-FR4) as one undoable edit. Refuses an
    /// eraser, a bad width or no points. Clears redo.
    pub fn add(
        &mut self,
        tool: Tool,
        color: InkColor,
        width: f32,
        points: Vec<InkPoint>,
        created_at_ms: u64,
        rec_ms: Option<u64>,
    ) -> InkResult<StrokeId> {
        let s = self.new_stroke(tool, color, width, points, created_at_ms, rec_ms)?;
        let id = s.id;
        self.strokes.push(s.clone());
        self.push(Edit::Add(s));
        Ok(id)
    }

    fn push(&mut self, e: Edit) {
        self.undo.push(e);
        self.redo.clear();
    }

    /// F21-FR1: a validated stroke with the next id, **not** added: the canvas's scene
    /// (`crate::canvas::Scene`) places it and keeps the one undo history for strokes and items.
    pub fn new_stroke(
        &mut self,
        tool: Tool,
        color: InkColor,
        width: f32,
        points: Vec<InkPoint>,
        created_at_ms: u64,
        rec_ms: Option<u64>,
    ) -> InkResult<Stroke> {
        if !tool.is_ink() {
            return Err(InkError::NonFinite("tool (an eraser draws nothing)"));
        }
        if !width.is_finite() || width <= 0.0 {
            return Err(InkError::NonFinite("stroke width"));
        }
        if points.is_empty() {
            return Err(InkError::EmptyStroke);
        }
        let id = StrokeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        Ok(Stroke {
            id,
            tool,
            color,
            width,
            points,
            created_at_ms,
            rec_ms,
        })
    }

    /// F21: the next stroke id, reserved (a restored stroke's new id, OQ34).
    pub fn next_id(&mut self) -> StrokeId {
        let id = StrokeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// F21-FR1: take the strokes in `ids` out, outside this layer's own history; each with the
    /// index it had, ascending.
    pub fn take(&mut self, ids: &[StrokeId]) -> Vec<(usize, Stroke)> {
        let mut out = Vec::new();
        let mut index = 0;
        self.strokes.retain(|s| {
            let gone = ids.contains(&s.id);
            if gone {
                out.push((index, s.clone()));
            }
            index += 1;
            !gone
        });
        out
    }

    /// F21-FR1: put strokes back at their indices (ascending), outside this layer's history.
    /// New ids continue past any stroke put back.
    pub fn put(&mut self, at: &[(usize, Stroke)]) {
        for (index, s) in at {
            let i = (*index).min(self.strokes.len());
            self.strokes.insert(i, s.clone());
            self.next_id = self.next_id.max(s.id.0.saturating_add(1));
        }
    }

    /// The strokes within `radius` of `(x, y)`, bottom to top. Non-destructive.
    #[must_use]
    pub fn strokes_hit(&self, x: f32, y: f32, radius: f32) -> Vec<StrokeId> {
        self.strokes
            .iter()
            .filter(|s| stroke_hit(s, x, y, radius))
            .map(|s| s.id)
            .collect()
    }

    /// The topmost stroke within `radius` of `(x, y)` (F17-FR7's tap).
    #[must_use]
    pub fn top_hit(&self, x: f32, y: f32, radius: f32) -> Option<&Stroke> {
        self.strokes
            .iter()
            .rev()
            .find(|s| stroke_hit(s, x, y, radius))
    }

    /// Erase every stroke an eraser sweep through `path` touches within `radius`, as one
    /// undoable edit. Returns the removed ids, in paint order.
    pub fn erase_path(&mut self, path: &[(f32, f32)], radius: f32) -> Vec<StrokeId> {
        let ids = self.hit_by_path(path, radius);
        self.delete_strokes(&ids)
    }

    /// The strokes an eraser sweep through `path` would take, in paint order.
    #[must_use]
    pub fn hit_by_path(&self, path: &[(f32, f32)], radius: f32) -> Vec<StrokeId> {
        self.strokes
            .iter()
            .filter(|s| {
                path.iter().any(|&(x, y)| stroke_hit(s, x, y, radius))
                    || path
                        .windows(2)
                        .any(|w| crosses_segment(s, w[0], w[1], radius))
            })
            .map(|s| s.id)
            .collect()
    }

    /// Delete the strokes in `ids` as one undoable edit; the removed ids in paint order.
    pub fn delete_strokes(&mut self, ids: &[StrokeId]) -> Vec<StrokeId> {
        let mut removed = Vec::new();
        let mut index = 0;
        self.strokes.retain(|s| {
            let gone = ids.contains(&s.id);
            if gone {
                removed.push((index, s.clone()));
            }
            index += 1;
            !gone
        });
        let out: Vec<StrokeId> = removed.iter().map(|(_, s)| s.id).collect();
        if !removed.is_empty() {
            self.push(Edit::Remove(removed));
        }
        out
    }

    /// Move the strokes in `ids` by `(dx, dy)` world units, as one undoable edit. Nothing is
    /// clamped: the canvas has no edge (R20). `false` when nothing moved.
    pub fn move_strokes(&mut self, ids: &[StrokeId], dx: f32, dy: f32) -> bool {
        if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        let present: Vec<StrokeId> = self
            .strokes
            .iter()
            .filter(|s| ids.contains(&s.id))
            .map(|s| s.id)
            .collect();
        if present.is_empty() {
            return false;
        }
        self.translate(&present, dx, dy);
        self.push(Edit::Move {
            ids: present,
            dx,
            dy,
        });
        true
    }

    fn translate(&mut self, ids: &[StrokeId], dx: f32, dy: f32) {
        for s in self.strokes.iter_mut().filter(|s| ids.contains(&s.id)) {
            for p in &mut s.points {
                p.x += dx;
                p.y += dy;
            }
        }
    }

    /// Whether an undo is available.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a redo is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the latest edit; the area it changed, or `None` when there was nothing to undo.
    pub fn undo(&mut self) -> Option<Option<BBox>> {
        let edit = self.undo.pop()?;
        let area = self.area_of(&edit);
        match &edit {
            Edit::Add(s) => self.strokes.retain(|x| x.id != s.id),
            Edit::Remove(removed) => {
                for (index, s) in removed {
                    let at = (*index).min(self.strokes.len());
                    self.strokes.insert(at, s.clone());
                }
            }
            Edit::Move { ids, dx, dy } => self.translate(ids, -dx, -dy),
        }
        let area = union_opt(area, self.area_of(&edit));
        self.redo.push(edit);
        Some(area)
    }

    /// Redo the latest undone edit; the area it changed, or `None` when there was none.
    pub fn redo(&mut self) -> Option<Option<BBox>> {
        let edit = self.redo.pop()?;
        let area = self.area_of(&edit);
        match &edit {
            Edit::Add(s) => self.strokes.push(s.clone()),
            Edit::Remove(removed) => {
                self.strokes
                    .retain(|x| !removed.iter().any(|(_, s)| s.id == x.id));
            }
            Edit::Move { ids, dx, dy } => self.translate(ids, *dx, *dy),
        }
        let area = union_opt(area, self.area_of(&edit));
        self.undo.push(edit);
        Some(area)
    }

    /// Where `edit`'s strokes are now (for a move, before and after are unioned by the caller).
    fn area_of(&self, edit: &Edit) -> Option<BBox> {
        match edit {
            Edit::Add(s) => Some(s.bounds()),
            Edit::Remove(removed) => removed.iter().map(|(_, s)| s.bounds()).reduce(BBox::union),
            Edit::Move { ids, .. } => self.bounds_of(ids),
        }
    }

    /// The union of the strokes in `ids`, or `None` when none is present.
    #[must_use]
    pub fn bounds_of(&self, ids: &[StrokeId]) -> Option<BBox> {
        self.strokes
            .iter()
            .filter(|s| ids.contains(&s.id))
            .map(Stroke::bounds)
            .reduce(BBox::union)
    }

    /// The union of every stroke (Fit, EC21), or `None` when empty.
    #[must_use]
    pub fn bounds(&self) -> Option<BBox> {
        self.strokes.iter().map(Stroke::bounds).reduce(BBox::union)
    }

    /// The strokes whose bounds meet `rect`, in paint order (what a view of it draws).
    #[must_use]
    pub fn strokes_in(&self, rect: BBox) -> Vec<&Stroke> {
        self.strokes
            .iter()
            .filter(|s| s.bounds().meets(rect))
            .collect()
    }
}

fn union_opt(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

/// Whether any segment of `stroke` (or its only point) is within `radius` of `(x, y)`.
fn stroke_hit(stroke: &Stroke, x: f32, y: f32, radius: f32) -> bool {
    let r2 = radius * radius;
    let pts = &stroke.points;
    if pts.len() == 1 {
        return dist2_point(pts[0].x, pts[0].y, x, y) <= r2;
    }
    pts.windows(2)
        .any(|w| dist2_to_segment(x, y, w[0].x, w[0].y, w[1].x, w[1].y) <= r2)
}

/// Whether the eraser's segment `a`→`b` crosses one of `stroke`'s segments: a fast sweep
/// whose samples land either side of a thin stroke still takes it.
fn crosses_segment(stroke: &Stroke, a: (f32, f32), b: (f32, f32), radius: f32) -> bool {
    stroke.points.windows(2).any(|w| {
        crate::ink::select::segments_intersect(
            (a.0, a.1),
            (b.0, b.1),
            (w[0].x, w[0].y),
            (w[1].x, w[1].y),
        )
    }) || stroke
        .points
        .iter()
        .any(|p| dist2_to_segment(p.x, p.y, a.0, a.1, b.0, b.1) <= radius * radius)
}

fn dist2_point(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// Squared distance from `(px, py)` to the segment `(ax, ay)`–`(bx, by)`.
fn dist2_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len2 = abx * abx + aby * aby;
    if len2 <= f32::EPSILON {
        return dist2_point(px, py, ax, ay);
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len2).clamp(0.0, 1.0);
    dist2_point(px, py, ax + t * abx, ay + t * aby)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn pt(x: f32, y: f32) -> InkPoint {
        InkPoint::new(x, y, 1.0, None, None, 0).unwrap()
    }

    fn drawn(l: &mut InkLayer, pts: &[(f32, f32)]) -> StrokeId {
        let points = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        l.add(Tool::Pen, InkColor::BLACK, 2.0, points, 0, Some(5))
            .unwrap()
    }

    fn ids(l: &InkLayer) -> Vec<u32> {
        l.strokes().iter().map(|s| s.id.0).collect()
    }

    #[test]
    fn world_points_are_not_clamped_but_must_be_finite() {
        let p = InkPoint::new(-12_000.5, 48_000.0, 3.0, Some(f32::NAN), Some(0.4), 9).unwrap();
        assert_eq!((p.x, p.y, p.pressure), (-12_000.5, 48_000.0, 1.0));
        assert_eq!((p.tilt_x, p.tilt_y, p.timestamp_ms), (None, Some(0.4), 9));
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(InkPoint::new(bad, 0.0, 1.0, None, None, 0).is_err());
            assert!(InkPoint::new(0.0, bad, 1.0, None, None, 0).is_err());
            assert!(InkPoint::new(0.0, 0.0, bad, None, None, 0).is_err());
        }
    }

    #[test]
    fn tools_and_colours() {
        for t in [Tool::Pen, Tool::Highlighter, Tool::Eraser] {
            assert_eq!(Tool::from_code(t.code()), Some(t));
        }
        assert_eq!(Tool::from_code(7), None);
        assert!(Tool::Highlighter.is_ink() && !Tool::Eraser.is_ink());
        assert_eq!(InkColor::default(), InkColor::rgba(0, 0, 0, 255));
        for e in [
            InkError::NonFinite("x"),
            InkError::EmptyStroke,
            InkError::BadEncoding("y".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn add_refuses_bad_strokes_and_keeps_times() {
        let mut l = InkLayer::new();
        let p = vec![pt(1.0, 1.0)];
        assert!(
            l.add(Tool::Eraser, InkColor::BLACK, 2.0, p.clone(), 0, None)
                .is_err()
        );
        assert!(
            l.add(Tool::Pen, InkColor::BLACK, 0.0, p.clone(), 0, None)
                .is_err()
        );
        assert!(
            l.add(Tool::Pen, InkColor::BLACK, f32::NAN, p.clone(), 0, None)
                .is_err()
        );
        assert_eq!(
            l.add(Tool::Pen, InkColor::BLACK, 2.0, vec![], 0, None),
            Err(InkError::EmptyStroke)
        );
        let id = l
            .add(Tool::Pen, InkColor::BLACK, 2.0, p, 1_700, Some(450_000))
            .unwrap();
        let s = l.get(id).unwrap();
        assert_eq!((s.created_at_ms, s.rec_ms), (1_700, Some(450_000)));
        assert!(l.get(StrokeId(99)).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn hit_testing_far_out_on_the_canvas() {
        let mut l = InkLayer::new();
        let a = drawn(&mut l, &[(50_000.0, -20_000.0), (50_100.0, -20_000.0)]);
        let b = drawn(&mut l, &[(50_050.0, -20_000.0)]); // a dot on top of a
        assert_eq!(l.strokes_hit(50_050.0, -19_999.0, 3.0), vec![a, b]);
        assert_eq!(l.top_hit(50_050.0, -19_999.0, 3.0).unwrap().id, b);
        assert_eq!(l.top_hit(50_010.0, -19_999.0, 3.0).unwrap().id, a);
        assert!(l.top_hit(0.0, 0.0, 3.0).is_none());
    }

    #[test]
    fn an_eraser_sweep_is_one_undo_step() {
        let mut l = InkLayer::new();
        drawn(&mut l, &[(0.0, 0.0), (100.0, 0.0)]); // 0: horizontal at y 0
        drawn(&mut l, &[(0.0, 50.0), (100.0, 50.0)]); // 1: horizontal at y 50
        drawn(&mut l, &[(500.0, 500.0), (600.0, 500.0)]); // 2: far away
        // Samples at y -30 and y 80 straddle both strokes: the segment between crosses them.
        let gone = l.erase_path(&[(40.0, -30.0), (40.0, 80.0)], 4.0);
        assert_eq!(gone, vec![StrokeId(0), StrokeId(1)]);
        assert_eq!(ids(&l), vec![2]);
        let area = l.undo().unwrap().unwrap();
        assert_eq!(ids(&l), vec![0, 1, 2], "both back, in paint order");
        assert!(area.x0 <= 0.0 && area.y1 >= 50.0);
        assert!(l.redo().is_some());
        assert_eq!(ids(&l), vec![2]);
        assert!(l.erase_path(&[(-900.0, -900.0)], 4.0).is_empty());
        // A single sample near a point erases it; a sample near a sweep segment's end does too.
        drawn(&mut l, &[(700.0, 700.0)]);
        assert_eq!(l.erase_path(&[(701.0, 701.0)], 4.0), vec![StrokeId(3)]);
        drawn(&mut l, &[(800.0, 800.0), (800.0, 810.0)]);
        assert_eq!(
            l.erase_path(&[(780.0, 805.0), (799.0, 805.0)], 2.0),
            vec![StrokeId(4)]
        );
    }

    #[test]
    fn moves_are_unbounded_and_undoable() {
        let mut l = InkLayer::new();
        let a = drawn(&mut l, &[(10.0, 10.0), (20.0, 20.0)]);
        assert!(!l.move_strokes(&[a], 0.0, 0.0));
        assert!(!l.move_strokes(&[a], f32::NAN, 1.0));
        assert!(!l.move_strokes(&[StrokeId(9)], 5.0, 5.0));
        assert!(l.move_strokes(&[a], -10_000.0, 30_000.0));
        assert_eq!(l.strokes()[0].points[0].x, -9_990.0);
        let area = l.undo().unwrap().unwrap();
        assert_eq!(l.strokes()[0].points[0].x, 10.0);
        assert!(area.x0 < -9_000.0 && area.x1 >= 20.0, "before and after");
        l.redo();
        assert_eq!(l.strokes()[0].points[1].y, 30_020.0);
    }

    #[test]
    fn undo_and_redo_of_add_and_delete() {
        let mut l = InkLayer::new();
        assert_eq!(l.undo(), None);
        assert_eq!(l.redo(), None);
        let a = drawn(&mut l, &[(0.0, 0.0), (1.0, 1.0)]);
        let b = drawn(&mut l, &[(5.0, 5.0), (6.0, 6.0)]);
        let c = drawn(&mut l, &[(9.0, 9.0), (8.0, 8.0)]);
        assert_eq!(l.delete_strokes(&[c, a]), vec![a, c]);
        assert_eq!(ids(&l), vec![1]);
        assert!(l.delete_strokes(&[StrokeId(42)]).is_empty());
        assert!(l.can_undo() && !l.can_redo());
        l.undo();
        assert_eq!(ids(&l), vec![0, 1, 2], "each back at its own index");
        l.undo(); // c's add
        l.undo(); // b's add
        assert_eq!(ids(&l), vec![0]);
        assert!(l.can_redo());
        l.redo();
        assert_eq!(l.strokes()[1].id, b);
        drawn(&mut l, &[(3.0, 3.0)]);
        assert!(!l.can_redo(), "a new stroke forks the timeline");
    }

    #[test]
    fn bounds_and_drawing_rects() {
        let mut l = InkLayer::new();
        assert_eq!(l.bounds(), None);
        let a = drawn(&mut l, &[(-100.0, -50.0), (-90.0, -40.0)]);
        let b = drawn(&mut l, &[(1_000.0, 2_000.0)]);
        let all = l.bounds().unwrap();
        assert_eq!(
            (all.x0, all.y0, all.x1, all.y1),
            (-101.0, -51.0, 1_001.0, 2_001.0)
        );
        assert_eq!(l.bounds_of(&[b]).unwrap().x0, 999.0);
        assert_eq!(l.bounds_of(&[]), None);
        let view = BBox {
            x0: -200.0,
            y0: -200.0,
            x1: 0.0,
            y1: 0.0,
        };
        let seen: Vec<StrokeId> = l.strokes_in(view).iter().map(|s| s.id).collect();
        assert_eq!(seen, vec![a]);
        let next = InkLayer::from_strokes(l.strokes().to_vec());
        let mut next = next;
        assert_eq!(drawn(&mut next, &[(0.0, 0.0)]), StrokeId(2));
    }
}
