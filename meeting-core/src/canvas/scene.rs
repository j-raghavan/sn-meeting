// The undo semantics follow sn-canvas `android/app/src/main/java/com/sncanvas/canvas/
// EditHistory.kt` (snapshots of the content, never of the viewport) and the paint order its
// element list keeps; MIT, Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
//! F21-FR1: one canvas's **scene**, the ink layer (`canvas.inkbin` v2, unchanged) plus the
//! items, per-stroke extras and paint order of `items.json`, with **one undo history** for
//! both. Every user gesture is one step: a stroke, an eraser sweep, a move, a restyle, a
//! group, a Clear. Pans and zooms are never steps. The history lives for the session and
//! keeps the last [`HISTORY_CAP`] steps.
//!
//! A pen stroke is a cheap step (the stroke and its place at the top of the paint order); any
//! step that touches items, extras or the order keeps the item side before and after.
use super::geom::{self, Pt, Shape};
use super::item::{Item, Ref, StrokeExtra};
use crate::ink::{BBox, InkLayer, Stroke, StrokeId};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// How far a duplicate lands from its original, down and right (`CanvasActions.kt:32`).
pub const DUPLICATE_OFFSET: f64 = 24.0;

/// The steps Undo can go back (F21-FR1 [Recommendation]).
pub const HISTORY_CAP: usize = 200;
/// `items.json`'s version.
pub const ITEMS_VERSION: u64 = 1;
/// `items.json` bigger than this is not ours (Data Model caps).
pub const MAX_ITEMS_BYTES: u64 = 16 * 1024 * 1024;
/// At most this many items load.
pub const MAX_ITEMS: usize = 100_000;

/// The item side of a scene: what `items.json` holds besides its envelope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub items: Vec<Item>,
    pub extras: BTreeMap<u32, StrokeExtra>,
    /// Every stroke and item exactly once, bottom to top.
    pub order: Vec<Ref>,
}

/// Strokes with their indices in paint order (ascending).
type Placed = Vec<(usize, Stroke)>;

/// One undoable step.
#[derive(Clone, Debug)]
struct Step {
    /// Strokes taken out, with their indices before (ascending).
    removed: Vec<(usize, Stroke)>,
    /// Strokes put in, with their indices after (ascending).
    added: Vec<(usize, Stroke)>,
    /// The item side before and after, when the step changed more than [`Step::pushed`].
    docs: Option<Box<(Doc, Doc)>>,
    /// Refs only appended to the order (a new stroke).
    pushed: Vec<Ref>,
}

/// What loading `items.json` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Entries dropped for breaking the caps (Security).
    pub dropped: usize,
    /// References to strokes no longer in `.inkbin` dropped (a build without F21 erased them).
    pub stale: usize,
}

/// One canvas: strokes and items, their paint order and the session's history.
#[derive(Clone, Debug)]
pub struct Scene {
    ink: InkLayer,
    doc: Doc,
    /// `items.json`'s envelope fields other than `version`, `order`, `items` and `strokes`.
    meta: Map<String, Value>,
    /// False when `items.json` couldn't be read: item edits are refused and the file is
    /// never written (F21-FR1, as the `.inkbin` rule).
    items_ok: bool,
    undo: Vec<Step>,
    redo: Vec<Step>,
    strokes_dirty: bool,
    /// `items.json` as last read or written; `None` when there was none.
    saved_items: Option<Vec<u8>>,
    /// The current selection (not an undo step).
    pub selection: Vec<Ref>,
}

/// Whether two refs name the same thing.
fn has(refs: &[Ref], r: &Ref) -> bool {
    refs.contains(r)
}

impl Scene {
    /// A scene of `ink` and the bytes of its `items.json`: `Ok(None)` when there is none,
    /// `Err` when it couldn't be read (item edits are then refused).
    #[must_use]
    pub fn load(ink: InkLayer, items: Result<Option<Vec<u8>>, String>) -> (Scene, LoadReport) {
        let mut s = Scene {
            ink,
            doc: Doc::default(),
            meta: Map::new(),
            items_ok: true,
            undo: Vec::new(),
            redo: Vec::new(),
            strokes_dirty: false,
            saved_items: None,
            selection: Vec::new(),
        };
        let mut report = LoadReport::default();
        match items {
            Ok(None) => {}
            Ok(Some(bytes)) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(o))
                    if o.get("version").and_then(Value::as_u64) == Some(ITEMS_VERSION) =>
                {
                    report = s.read(o);
                    s.saved_items = Some(bytes);
                }
                _ => s.items_ok = false,
            },
            Err(_) => s.items_ok = false,
        }
        s.normalise();
        (s, report)
    }

    /// Fill the item side from a parsed `items.json` v1.
    fn read(&mut self, mut o: Map<String, Value>) -> LoadReport {
        let mut r = LoadReport::default();
        let strokes: HashSet<u32> = self.ink.strokes().iter().map(|s| s.id.0).collect();
        let mut seen = HashSet::new();
        if let Some(Value::Array(items)) = o.remove("items") {
            for v in items {
                match Item::from_value(&v) {
                    Some(it) if self.doc.items.len() < MAX_ITEMS && seen.insert(it.id.clone()) => {
                        self.doc.items.push(it);
                    }
                    _ => r.dropped += 1,
                }
            }
        }
        if let Some(Value::Object(extras)) = o.remove("strokes") {
            for (k, v) in extras {
                match k.parse::<u32>() {
                    Ok(id) if strokes.contains(&id) => {
                        let e = StrokeExtra::from_value(&v);
                        if !e.is_empty() {
                            self.doc.extras.insert(id, e);
                        }
                    }
                    Ok(_) => r.stale += 1,
                    Err(_) => r.dropped += 1,
                }
            }
        }
        if let Some(Value::Array(order)) = o.remove("order") {
            for v in order {
                match v.as_str().and_then(Ref::parse) {
                    Some(Ref::Stroke(n)) if !strokes.contains(&n) => r.stale += 1,
                    Some(rf) => self.doc.order.push(rf),
                    None => r.dropped += 1,
                }
            }
        }
        // A bound end whose shape is gone keeps its stored point and loses the binding.
        let items: HashSet<String> = self.doc.items.iter().map(|i| i.id.clone()).collect();
        for it in &mut self.doc.items {
            for end in [it.start.as_mut(), it.end.as_mut()].into_iter().flatten() {
                let gone = match &end.bind {
                    Some(Ref::Stroke(n)) => !strokes.contains(n),
                    Some(Ref::Item(i)) => !items.contains(i) || *i == it.id,
                    None => false,
                };
                if gone {
                    end.bind = None;
                    r.stale += 1;
                }
            }
        }
        o.remove("version");
        self.meta = o;
        r
    }

    /// Make `order` name every stroke and item exactly once: its own entries first (unknown
    /// and repeated ones dropped), then whatever it misses, strokes then items, in file order.
    fn normalise(&mut self) {
        let mut seen = HashSet::new();
        let present: HashSet<Ref> = self.all_refs().into_iter().collect();
        self.doc
            .order
            .retain(|r| present.contains(r) && seen.insert(r.clone()));
        for r in self.all_refs() {
            if !seen.contains(&r) {
                self.doc.order.push(r);
            }
        }
    }

    fn all_refs(&self) -> Vec<Ref> {
        self.ink
            .strokes()
            .iter()
            .map(|s| Ref::Stroke(s.id.0))
            .chain(self.doc.items.iter().map(|i| Ref::Item(i.id.clone())))
            .collect()
    }

    /// The ink layer (strokes in `.inkbin` order).
    #[must_use]
    pub fn ink(&self) -> &InkLayer {
        &self.ink
    }

    /// The item side.
    #[must_use]
    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    /// The item side, for a step's edit.
    pub(crate) fn doc_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }

    /// Whether `items.json` was readable (item edits allowed).
    #[must_use]
    pub fn items_ok(&self) -> bool {
        self.items_ok
    }

    /// Whether the canvas holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ink.is_empty() && self.doc.items.is_empty()
    }

    /// The item `id`.
    #[must_use]
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.doc.items.iter().find(|i| i.id == id)
    }

    /// Whether the ref names something on this canvas.
    #[must_use]
    pub fn exists(&self, r: &Ref) -> bool {
        match r {
            Ref::Stroke(n) => self.ink.get(StrokeId(*n)).is_some(),
            Ref::Item(i) => self.item(i).is_some(),
        }
    }

    // ---- history ------------------------------------------------------------------------

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

    fn record(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Run `f` as one step: it may change strokes through the returned lists and the item
    /// side freely. Nothing changed records nothing. The area changed, or `None`.
    pub(crate) fn step(&mut self, f: impl FnOnce(&mut Scene) -> (Placed, Placed)) -> Option<BBox> {
        let before = self.doc.clone();
        let (mut removed, mut added) = f(self);
        if removed == added {
            // Strokes taken out and put back unchanged: no change to record.
            (removed, added) = (Vec::new(), Vec::new());
        }
        let docs = (self.doc != before).then(|| Box::new((before, self.doc.clone())));
        if removed.is_empty() && added.is_empty() && docs.is_none() {
            return None;
        }
        let step = Step {
            removed,
            added,
            docs,
            pushed: Vec::new(),
        };
        if !step.removed.is_empty() || !step.added.is_empty() {
            self.strokes_dirty = true;
        }
        let area = self.area_of(&step);
        self.record(step);
        self.prune_selection();
        Some(area.unwrap_or(BBox {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 0.0,
        }))
    }

    /// Undo the latest step; the area it changed (`Some(None)`: nothing to draw), or `None`
    /// when there was nothing to undo.
    pub fn undo(&mut self) -> Option<Option<BBox>> {
        let s = self.undo.pop()?;
        let before = self.area_of(&s);
        self.ink.take(&ids_of(&s.added));
        self.ink.put(&s.removed);
        match &s.docs {
            Some(d) => self.doc = d.0.clone(),
            None => {
                let n = self.doc.order.len().saturating_sub(s.pushed.len());
                self.doc.order.truncate(n);
            }
        }
        self.strokes_dirty |= !s.removed.is_empty() || !s.added.is_empty();
        let area = union_opt(before, self.area_of(&s));
        self.redo.push(s);
        self.prune_selection();
        Some(area)
    }

    /// Redo the latest undone step, as [`Self::undo`].
    pub fn redo(&mut self) -> Option<Option<BBox>> {
        let s = self.redo.pop()?;
        let before = self.area_of(&s);
        self.ink.take(&ids_of(&s.removed));
        self.ink.put(&s.added);
        match &s.docs {
            Some(d) => self.doc = d.1.clone(),
            None => self.doc.order.extend(s.pushed.iter().cloned()),
        }
        self.strokes_dirty |= !s.removed.is_empty() || !s.added.is_empty();
        let area = union_opt(before, self.area_of(&s));
        self.undo.push(s);
        self.prune_selection();
        Some(area)
    }

    fn prune_selection(&mut self) {
        let sel = std::mem::take(&mut self.selection);
        self.selection = sel.into_iter().filter(|r| self.exists(r)).collect();
    }

    /// Where a step's strokes and items are now (the caller unions before and after).
    fn area_of(&self, s: &Step) -> Option<BBox> {
        let mut area = s
            .removed
            .iter()
            .chain(&s.added)
            .map(|(_, st)| st.bounds())
            .reduce(BBox::union);
        if let Some(d) = &s.docs {
            let (a, b) = (&d.0.items, &d.1.items);
            for it in a.iter().chain(b) {
                let same = a.iter().find(|x| x.id == it.id) == b.iter().find(|x| x.id == it.id);
                if !same || self.bound_to_change(it, s) {
                    area = union_opt(area, self.item_bounds(it));
                }
            }
            for (id, _) in d.0.extras.iter().chain(&d.1.extras) {
                area = union_opt(area, self.ink.get(StrokeId(*id)).map(Stroke::bounds));
            }
        }
        area
    }

    /// Whether `it` is an arrow bound to a stroke the step changed (its route moved too).
    fn bound_to_change(&self, it: &Item, s: &Step) -> bool {
        let changed: Vec<u32> = s
            .removed
            .iter()
            .chain(&s.added)
            .map(|(_, x)| x.id.0)
            .collect();
        [&it.start, &it.end]
            .into_iter()
            .flatten()
            .any(|e| matches!(e.bind, Some(Ref::Stroke(n)) if changed.contains(&n)))
    }

    // ---- geometry ------------------------------------------------------------------------

    /// The box an arrow end bound to `r` attaches to.
    #[must_use]
    pub fn shape_of(&self, r: &Ref) -> Option<Shape> {
        match r {
            Ref::Stroke(n) => self
                .ink
                .get(StrokeId(*n))
                .map(|s| Shape::of_box(s.bounds())),
            Ref::Item(i) => self.item(i).filter(|it| !it.has_ends()).map(Shape::of_item),
        }
    }

    /// A line's or arrow's ends now (bound ends resolved).
    #[must_use]
    pub fn ends(&self, it: &Item) -> Option<(Pt, Pt)> {
        let (s, e) = (it.start.as_ref()?, it.end.as_ref()?);
        let shape = |b: &Option<Ref>| b.as_ref().and_then(|r| self.shape_of(r));
        Some(geom::resolve_ends(
            (Pt::new(s.x, s.y), shape(&s.bind)),
            (Pt::new(e.x, e.y), shape(&e.bind)),
        ))
    }

    /// An item's world bounds, grown by its stroke width.
    #[must_use]
    pub fn item_bounds(&self, it: &Item) -> Option<BBox> {
        let pts = match self.ends(it) {
            Some((a, b)) => vec![a, b],
            None => geom::corners(it).to_vec(),
        };
        let m = it.style.stroke_width() / 2.0;
        geom::bounds_of(&pts).map(|b| BBox {
            x0: b.x0 - m,
            y0: b.y0 - m,
            x1: b.x1 + m,
            y1: b.y1 + m,
        })
    }

    /// The bounds of `r`.
    #[must_use]
    pub fn bounds_of_ref(&self, r: &Ref) -> Option<BBox> {
        match r {
            Ref::Stroke(n) => self.ink.get(StrokeId(*n)).map(Stroke::bounds),
            Ref::Item(i) => self.item(i).and_then(|it| self.item_bounds(it)),
        }
    }

    /// The union of `refs`' bounds.
    #[must_use]
    pub fn bounds_of(&self, refs: &[Ref]) -> Option<BBox> {
        refs.iter()
            .filter_map(|r| self.bounds_of_ref(r))
            .reduce(BBox::union)
    }

    /// The union of everything (Fit), or `None` when empty.
    #[must_use]
    pub fn bounds(&self) -> Option<BBox> {
        self.bounds_of(&self.doc.order)
    }

    /// The paint order, bottom to top.
    #[must_use]
    pub fn order(&self) -> &[Ref] {
        &self.doc.order
    }

    // ---- edits -----------------------------------------------------------------------------

    /// F21-FR1 / F17-FR4: commit a pen or highlighter stroke as one step, at the top of the
    /// paint order, with its style ids when given (F21-FR7).
    pub fn add_stroke(&mut self, stroke: Stroke, style: Option<super::style::Style>) -> StrokeId {
        let id = stroke.id;
        let index = self.ink.strokes().len();
        self.ink.put(&[(index, stroke.clone())]);
        self.strokes_dirty = true;
        let r = Ref::Stroke(id.0);
        match style {
            None => {
                self.doc.order.push(r.clone());
                self.record(Step {
                    removed: Vec::new(),
                    added: vec![(index, stroke)],
                    docs: None,
                    pushed: vec![r],
                });
            }
            Some(st) => {
                let before = self.doc.clone();
                self.doc.order.push(r);
                self.doc.extras.insert(
                    id.0,
                    StrokeExtra {
                        style: Some(st),
                        ..StrokeExtra::default()
                    },
                );
                self.record(Step {
                    removed: Vec::new(),
                    added: vec![(index, stroke)],
                    docs: Some(Box::new((before, self.doc.clone()))),
                    pushed: Vec::new(),
                });
            }
        }
        id
    }

    /// A stroke for this scene with the next id (validated; not yet added).
    pub fn new_stroke(
        &mut self,
        tool: crate::ink::Tool,
        color: crate::ink::InkColor,
        width: f32,
        points: Vec<crate::ink::InkPoint>,
        created_at_ms: u64,
        rec_ms: Option<u64>,
    ) -> crate::ink::model::InkResult<Stroke> {
        self.ink
            .new_stroke(tool, color, width, points, created_at_ms, rec_ms)
    }

    /// Delete `refs` as one step (strokes, and items when they can be edited). Their order
    /// entries and extras go with them. The refs removed, in paint order.
    pub fn delete(&mut self, refs: &[Ref]) -> (Vec<Ref>, Option<BBox>) {
        let refs: Vec<Ref> = self
            .doc
            .order
            .iter()
            .filter(|r| has(refs, r) && (self.items_ok || matches!(r, Ref::Stroke(_))))
            .cloned()
            .collect();
        let area = self.step(|s| {
            let ids: Vec<StrokeId> = refs.iter().filter_map(Ref::stroke).collect();
            s.remove_from_doc(&refs);
            let removed = s.ink.take(&ids);
            (removed, Vec::new())
        });
        (refs, area)
    }

    /// Take `refs` out of the item side: items, extras and order entries. An arrow end bound
    /// to something removed is freed at its last centre (`CanvasCore.kt:347-372`).
    fn remove_from_doc(&mut self, refs: &[Ref]) {
        let centres: Vec<(Ref, Pt)> = refs
            .iter()
            .filter_map(|r| self.shape_of(r).map(|s| (r.clone(), s.centre())))
            .collect();
        for it in &mut self.doc.items {
            for e in [it.start.as_mut(), it.end.as_mut()].into_iter().flatten() {
                if let Some((_, c)) = centres.iter().find(|(r, _)| Some(r) == e.bind.as_ref()) {
                    (e.x, e.y, e.bind) = (c.x, c.y, None);
                }
            }
        }
        self.doc.order.retain(|r| !has(refs, r));
        self.doc
            .items
            .retain(|i| !has(refs, &Ref::Item(i.id.clone())));
        self.doc
            .extras
            .retain(|id, _| !has(refs, &Ref::Stroke(*id)));
    }

    /// Move `refs` by `(dx, dy)` world units as one step. A line or arrow moves by its free
    /// ends; an end bound to a shape follows the shape anyway (`CanvasCore.moveElements`).
    pub fn move_refs(&mut self, refs: &[Ref], dx: f64, dy: f64) -> Option<BBox> {
        if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
            return None;
        }
        let refs: Vec<Ref> = refs
            .iter()
            .filter(|r| self.exists(r) && (self.items_ok || matches!(r, Ref::Stroke(_))))
            .cloned()
            .collect();
        self.step(|s| {
            let (removed, added) = s.map_strokes(&refs, |st| {
                #[allow(clippy::cast_possible_truncation)] // world coordinates fit f32
                let (fx, fy) = (dx as f32, dy as f32);
                for p in &mut st.points {
                    p.x += fx;
                    p.y += fy;
                }
            });
            for it in &mut s.doc.items {
                if !has(&refs, &Ref::Item(it.id.clone())) {
                    continue;
                }
                it.x += dx;
                it.y += dy;
                for e in [it.start.as_mut(), it.end.as_mut()].into_iter().flatten() {
                    e.x += dx;
                    e.y += dy;
                }
            }
            (removed, added)
        })
    }

    /// One step changing the item `id` by `f` (nothing changed: no step).
    pub(crate) fn edit_item(&mut self, id: &str, f: impl FnOnce(&mut Item)) -> Option<BBox> {
        self.step(|s| {
            if let Some(it) = s.doc.items.iter_mut().find(|i| i.id == id) {
                f(it);
            }
            (Vec::new(), Vec::new())
        })
    }

    /// One step changing only the item side (`items`, `extras`, `order`).
    pub(crate) fn step_doc(&mut self, f: impl FnOnce(&mut Doc)) -> Option<BBox> {
        self.step(|s| {
            f(&mut s.doc);
            (Vec::new(), Vec::new())
        })
    }

    /// One step moving every point of the strokes in `refs` through `f` (world in, world
    /// out); pressure and times are kept.
    pub(crate) fn step_strokes(
        &mut self,
        refs: &[Ref],
        f: impl Fn(Pt) -> (f64, f64),
    ) -> Option<BBox> {
        self.step(|s| {
            s.map_strokes(refs, |st| {
                for p in &mut st.points {
                    let (x, y) = f(Pt::new(f64::from(p.x), f64::from(p.y)));
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        (p.x, p.y) = (x as f32, y as f32);
                    }
                }
            })
        })
    }

    /// Replace each stroke in `refs` by `f` of it, keeping its id and index: the lists a
    /// step records.
    pub(crate) fn map_strokes(
        &mut self,
        refs: &[Ref],
        f: impl Fn(&mut Stroke),
    ) -> (Placed, Placed) {
        let ids: Vec<StrokeId> = refs.iter().filter_map(Ref::stroke).collect();
        let removed = self.ink.take(&ids);
        let added: Vec<(usize, Stroke)> = removed
            .iter()
            .map(|(i, st)| {
                let mut n = st.clone();
                f(&mut n);
                (*i, n)
            })
            .collect();
        self.ink.put(&added);
        (removed, added)
    }

    /// The group `r` belongs to, if any.
    #[must_use]
    pub fn group_of(&self, r: &Ref) -> Option<&str> {
        match r {
            Ref::Stroke(n) => self.doc.extras.get(n).and_then(|e| e.group_id.as_deref()),
            Ref::Item(i) => self.item(i).and_then(|it| it.group_id.as_deref()),
        }
    }

    /// `refs` and every other member of their groups (groups don't nest; a member's whole
    /// group joins, `CanvasActions.kt:56-97`), in paint order.
    #[must_use]
    pub fn with_groups(&self, refs: &[Ref]) -> Vec<Ref> {
        let groups: Vec<&str> = refs.iter().filter_map(|r| self.group_of(r)).collect();
        self.doc
            .order
            .iter()
            .filter(|r| has(refs, r) || self.group_of(r).is_some_and(|g| groups.contains(&g)))
            .cloned()
            .collect()
    }

    /// F21-FR3: an eraser sweep through `path` with radius `r`: every stroke and item it
    /// touches (item outlines, `hit::touches`), and their groups, as one step. The refs
    /// erased, in paint order.
    pub fn erase(&mut self, path: &[(f32, f32)], radius: f32) -> (Vec<Ref>, Option<BBox>) {
        let mut hit: Vec<Ref> = self
            .ink
            .hit_by_path(path, radius)
            .into_iter()
            .map(|s| Ref::Stroke(s.0))
            .collect();
        if self.items_ok {
            let pts = super::hit::densify(path, radius);
            for it in &self.doc.items {
                let ends = self.ends(it);
                if pts
                    .iter()
                    .any(|p| super::hit::touches(it, ends, *p, f64::from(radius)))
                {
                    hit.push(Ref::Item(it.id.clone()));
                }
            }
        }
        let all = self.with_groups(&hit);
        self.delete(&all)
    }

    /// F21-FR3: Clear canvas: every stroke and item of this canvas out, as one step that Undo
    /// brings back identically (ids, times, order, groups, bindings, links). `None` when
    /// there was nothing to clear (or items can't be edited: the canvas is then left alone).
    pub fn clear(&mut self) -> Option<BBox> {
        if !self.items_ok || self.is_empty() {
            return None;
        }
        let all = self.doc.order.clone();
        self.delete(&all).1
    }

    /// OQ34: put a cleared canvas's content back (from its backup) on top of this one, as one
    /// step. Ids that are taken here get fresh ones, and bindings, groups and the order follow.
    pub fn absorb(&mut self, other: &Scene) -> Option<BBox> {
        if !self.items_ok || other.is_empty() {
            return None;
        }
        self.step(|s| {
            let mut strokes: HashMap<u32, u32> = HashMap::new();
            let mut added = Vec::new();
            for st in other.ink.strokes() {
                let mut n = st.clone();
                n.id = s.ink.next_id();
                strokes.insert(st.id.0, n.id.0);
                added.push((s.ink.strokes().len() + added.len(), n));
            }
            s.ink.put(&added);
            let mut items: HashMap<String, String> = HashMap::new();
            for it in &other.doc.items {
                let id = if s.item(&it.id).is_some() || items.values().any(|v| *v == it.id) {
                    s.fresh_id()
                } else {
                    it.id.clone()
                };
                items.insert(it.id.clone(), id);
            }
            let map = |r: &Ref| -> Option<Ref> {
                match r {
                    Ref::Stroke(n) => strokes.get(n).map(|m| Ref::Stroke(*m)),
                    Ref::Item(i) => items.get(i).map(|m| Ref::Item(m.clone())),
                }
            };
            for it in &other.doc.items {
                let mut n = it.clone();
                n.id.clone_from(&items[&it.id]);
                for e in [n.start.as_mut(), n.end.as_mut()].into_iter().flatten() {
                    e.bind = e.bind.as_ref().and_then(map);
                }
                s.doc.items.push(n);
            }
            for (id, e) in &other.doc.extras {
                s.doc.extras.insert(strokes[id], e.clone());
            }
            s.doc.order.extend(other.doc.order.iter().filter_map(map));
            (Vec::new(), added)
        })
    }

    /// An item id no item here has: 8 hex digits, as `items.json`'s.
    #[must_use]
    pub fn fresh_id(&self) -> String {
        loop {
            let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
            if self.item(&id).is_none() {
                return id;
            }
        }
    }

    /// F17 / F21-FR4: what a lasso loop takes: strokes it encloses or crosses, and items whose
    /// outline it encloses, in paint order (groups not yet expanded).
    #[must_use]
    pub fn lasso(&self, poly: &[(f32, f32)]) -> Vec<Ref> {
        let ids = crate::ink::select_in_polygon(&self.ink, poly);
        #[allow(clippy::cast_possible_truncation)] // world coordinates fit f32
        let inside = |p: &Pt| {
            poly.len() >= 3 && crate::ink::select::point_in_polygon(p.x as f32, p.y as f32, poly)
        };
        self.doc
            .order
            .iter()
            .filter(|r| match r {
                Ref::Stroke(n) => ids.contains(&StrokeId(*n)),
                Ref::Item(i) => self.item(i).is_some_and(|it| {
                    let pts = match self.ends(it) {
                        Some((a, b)) => vec![a, b],
                        None => geom::corners(it).to_vec(),
                    };
                    pts.iter().all(inside)
                }),
            })
            .cloned()
            .collect()
    }

    /// F21-FR4: the topmost stroke or item under a pen tap at `p` (within `r`): an item's box
    /// counts, inside and out, as a line's segment and a stroke's ink do.
    #[must_use]
    pub fn top_at(&self, p: Pt, r: f64) -> Option<Ref> {
        #[allow(clippy::cast_possible_truncation)]
        let hits = self.ink.strokes_hit(p.x as f32, p.y as f32, r as f32);
        self.doc
            .order
            .iter()
            .rev()
            .find(|x| match x {
                Ref::Stroke(n) => hits.contains(&StrokeId(*n)),
                Ref::Item(i) => self.item(i).is_some_and(|it| {
                    let ends = self.ends(it);
                    super::hit::touches(it, ends, p, r) || (ends.is_none() && geom::contains(it, p))
                }),
            })
            .cloned()
    }

    /// F21-FR4: everything a selection rectangle touches (its bounds meet the rectangle,
    /// `CanvasCore.elementsIn`), in paint order.
    #[must_use]
    pub fn in_rect(&self, rect: BBox) -> Vec<Ref> {
        self.doc
            .order
            .iter()
            .filter(|r| self.bounds_of_ref(r).is_some_and(|b| b.meets(rect)))
            .cloned()
            .collect()
    }

    /// F21-FR4: select `refs` and the rest of their groups (not an undo step).
    pub fn select(&mut self, refs: &[Ref]) {
        let refs: Vec<Ref> = refs.iter().filter(|r| self.exists(r)).cloned().collect();
        self.selection = self.with_groups(&refs);
    }

    /// F21-FR4: copies of `refs` 24 world units down and right, on top, with new ids and new
    /// groups (`CanvasActions.kt:32`), as one step; the copies become the selection. A copied
    /// arrow stays bound only to shapes copied with it; its other ends are frozen where they
    /// are now. Copies are untimed (OQ36). `created_at` stamps the new items.
    pub fn duplicate(&mut self, refs: &[Ref], created_at: &str) -> Option<BBox> {
        if !self.items_ok {
            return None;
        }
        let refs = self.with_groups(refs);
        if refs.is_empty() {
            return None;
        }
        let d = DUPLICATE_OFFSET;
        let mut copies = Vec::new();
        let area = self.step(|s| {
            let mut strokes: HashMap<u32, u32> = HashMap::new();
            let mut ids: HashMap<String, String> = HashMap::new();
            let mut groups: HashMap<String, String> = HashMap::new();
            let mut new_group = |s: &Scene, g: &Option<String>| -> Option<String> {
                g.as_ref().map(|g| {
                    groups
                        .entry(g.clone())
                        .or_insert_with(|| format!("g-{}", &s.fresh_id()[..4]))
                        .clone()
                })
            };
            let mut added = Vec::new();
            for r in &refs {
                if let Ref::Stroke(n) = r
                    && let Some(st) = s.ink.get(StrokeId(*n))
                {
                    let mut c = st.clone();
                    c.id = s.ink.next_id();
                    c.rec_ms = None;
                    #[allow(clippy::cast_possible_truncation)]
                    for p in &mut c.points {
                        p.x += d as f32;
                        p.y += d as f32;
                    }
                    strokes.insert(*n, c.id.0);
                    added.push((s.ink.strokes().len() + added.len(), c));
                }
            }
            for r in &refs {
                if let Ref::Item(i) = r {
                    let fresh = loop {
                        let f = s.fresh_id();
                        if !ids.values().any(|v| *v == f) {
                            break f;
                        }
                    };
                    ids.insert(i.clone(), fresh);
                }
            }
            let mut new_items = Vec::new();
            for r in &refs {
                let Ref::Item(i) = r else { continue };
                let Some(it) = s.item(i) else { continue };
                let mut c = it.clone();
                let now = s.ends(it);
                c.id.clone_from(&ids[i]);
                c.x += d;
                c.y += d;
                c.rec_ms = None;
                c.created_at = Some(created_at.to_owned());
                c.group_id = new_group(s, &it.group_id);
                if let (Some((a, b)), Some(st), Some(en)) = (now, c.start.as_mut(), c.end.as_mut())
                {
                    for (e, at) in [(st, a), (en, b)] {
                        let kept = match &e.bind {
                            Some(Ref::Item(t)) => ids.get(t).map(|n| Ref::Item(n.clone())),
                            Some(Ref::Stroke(t)) => strokes.get(t).map(|n| Ref::Stroke(*n)),
                            None => None,
                        };
                        if kept.is_none() {
                            (e.x, e.y) = (at.x + d, at.y + d);
                        } else {
                            (e.x, e.y) = (e.x + d, e.y + d);
                        }
                        e.bind = kept;
                    }
                }
                new_items.push(c);
            }
            s.ink.put(&added);
            for (old, new) in &strokes {
                if let Some(e) = s.doc.extras.get(old).cloned() {
                    let mut e = e;
                    e.group_id = new_group(s, &e.group_id);
                    s.doc.extras.insert(*new, e);
                }
            }
            for r in &refs {
                let c = match r {
                    Ref::Stroke(n) => Ref::Stroke(strokes[n]),
                    Ref::Item(i) => Ref::Item(ids[i].clone()),
                };
                s.doc.order.push(c.clone());
                copies.push(c);
            }
            s.doc.items.extend(new_items);
            (Vec::new(), added)
        });
        self.selection = copies;
        area
    }

    /// F21-FR11: every image file this canvas refers to now.
    #[must_use]
    pub fn image_files(&self) -> HashSet<String> {
        self.doc
            .items
            .iter()
            .filter_map(|it| it.image.as_ref().map(|i| i.file.clone()))
            .collect()
    }

    // ---- saving --------------------------------------------------------------------------

    /// Whether `.inkbin` changed since it was last saved.
    #[must_use]
    pub fn strokes_dirty(&self) -> bool {
        self.strokes_dirty
    }

    /// `.inkbin` was written.
    pub fn strokes_saved(&mut self) {
        self.strokes_dirty = false;
    }

    /// `items.json` as it would be written now.
    #[must_use]
    pub fn items_json(&self) -> Vec<u8> {
        let mut o = self.meta.clone();
        o.insert("version".into(), json!(ITEMS_VERSION));
        o.insert(
            "order".into(),
            Value::Array(
                self.doc
                    .order
                    .iter()
                    .map(|r| json!(r.to_string()))
                    .collect(),
            ),
        );
        o.insert(
            "items".into(),
            Value::Array(self.doc.items.iter().map(Item::to_value).collect()),
        );
        let strokes: Map<String, Value> = self
            .doc
            .extras
            .iter()
            .map(|(id, e)| (id.to_string(), e.to_value()))
            .collect();
        o.insert("strokes".into(), Value::Object(strokes));
        serde_json::to_vec(&Value::Object(o)).unwrap_or_default()
    }

    /// `items.json` to write now, or `None`: unreadable (never overwritten), unchanged, or
    /// not needed (no file yet, no items or extras, and the strokes in their own order).
    #[must_use]
    pub fn items_to_save(&self) -> Option<Vec<u8>> {
        if !self.items_ok {
            return None;
        }
        let bytes = self.items_json();
        match &self.saved_items {
            Some(b) if *b == bytes => None,
            Some(_) => Some(bytes),
            None => {
                let strokes = self.ink.strokes();
                let natural = self.doc.order.len() == strokes.len()
                    && self
                        .doc
                        .order
                        .iter()
                        .zip(strokes)
                        .all(|(r, s)| *r == Ref::Stroke(s.id.0));
                let plain =
                    self.doc.items.is_empty() && self.doc.extras.is_empty() && self.meta.is_empty();
                (!(natural && plain)).then_some(bytes)
            }
        }
    }

    /// `items.json` was written as `bytes`.
    pub fn items_saved(&mut self, bytes: Vec<u8>) {
        self.saved_items = Some(bytes);
    }

    /// Whether anything waits to be saved.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.strokes_dirty || self.items_to_save().is_some()
    }
}

fn ids_of(list: &[(usize, Stroke)]) -> Vec<StrokeId> {
    list.iter().map(|(_, s)| s.id).collect()
}

fn union_opt(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::canvas::item::{End, Link};
    use crate::canvas::style::Style;
    use crate::ink::{InkColor, InkPoint, Tool};

    pub(crate) fn pen(s: &mut Scene, pts: &[(f32, f32)], rec: Option<u64>) -> StrokeId {
        let points = pts
            .iter()
            .map(|&(x, y)| InkPoint::new(x, y, 0.5, None, None, 0).unwrap())
            .collect();
        let st = s
            .new_stroke(Tool::Pen, InkColor::BLACK, 2.0, points, 1, rec)
            .unwrap();
        s.add_stroke(st, None)
    }

    pub(crate) fn rect(id: &str, x: f64, y: f64) -> Item {
        let mut it = Item::new(id, "rectangle", Style::default());
        (it.x, it.y, it.width, it.height) = (x, y, 100.0, 50.0);
        it
    }

    /// Put an item in directly, as a loaded file would.
    pub(crate) fn with_item(s: &mut Scene, it: Item) {
        s.doc.order.push(Ref::Item(it.id.clone()));
        s.doc.items.push(it);
    }

    fn empty() -> Scene {
        Scene::load(InkLayer::new(), Ok(None)).0
    }

    fn order(s: &Scene) -> Vec<String> {
        s.order().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn strokes_are_steps_on_top_of_the_order() {
        let mut s = empty();
        assert!(s.is_empty() && s.items_ok());
        let a = pen(&mut s, &[(0.0, 0.0), (10.0, 0.0)], Some(5));
        with_item(&mut s, rect("r1", 50.0, 50.0));
        let b = pen(&mut s, &[(0.0, 9.0)], None);
        assert_eq!(order(&s), ["s:0", "i:r1", "s:1"]);
        assert_eq!((a, b), (StrokeId(0), StrokeId(1)));
        assert!(s.undo().is_some());
        assert_eq!(order(&s), ["s:0", "i:r1"]);
        assert!(s.redo().is_some());
        assert_eq!(order(&s), ["s:0", "i:r1", "s:1"]);
        assert!(s.can_undo() && !s.can_redo());
        // A styled stroke keeps its style ids beside it, and Undo takes them back.
        let points = vec![InkPoint::new(1.0, 1.0, 1.0, None, None, 0).unwrap()];
        let st = s
            .new_stroke(Tool::Pen, InkColor::BLACK, 3.0, points, 1, None)
            .unwrap();
        let c = s.add_stroke(st, Some(Style::default().with("color", "red")));
        assert_eq!(s.doc().extras[&c.0].style.as_ref().unwrap().color, "red");
        s.undo();
        assert!(s.doc().extras.is_empty());
        assert_eq!(order(&s), ["s:0", "i:r1", "s:1"]);
        s.redo();
        assert_eq!(order(&s).last().unwrap(), "s:2");
    }

    #[test]
    fn the_history_is_capped() {
        let mut s = empty();
        for i in 0..(HISTORY_CAP + 5) {
            #[allow(clippy::cast_precision_loss)]
            pen(&mut s, &[(i as f32, 0.0)], None);
        }
        let mut n = 0;
        while s.undo().is_some() {
            n += 1;
        }
        assert_eq!(n, HISTORY_CAP);
        assert_eq!(s.ink().strokes().len(), 5);
        assert_eq!(s.undo(), None);
        assert_eq!(s.redo().map(|a| a.is_some()), Some(true));
    }

    #[test]
    fn moving_and_deleting_strokes_and_items_together() {
        let mut s = empty();
        let a = pen(&mut s, &[(0.0, 0.0), (10.0, 0.0)], Some(9));
        with_item(&mut s, rect("r1", 0.0, 100.0));
        let refs = vec![Ref::Stroke(a.0), Ref::Item("r1".into())];
        assert!(s.move_refs(&refs, 0.0, 0.0).is_none());
        assert!(s.move_refs(&refs, f64::NAN, 1.0).is_none());
        assert!(s.move_refs(&[Ref::Item("zz".into())], 5.0, 5.0).is_none());
        let area = s.move_refs(&refs, 100.0, -50.0).unwrap();
        assert!(
            area.x0 <= 0.0 && area.x1 >= 200.0,
            "before and after: {area:?}"
        );
        assert_eq!(s.ink().get(a).unwrap().points[1].x, 110.0);
        assert_eq!(s.ink().get(a).unwrap().rec_ms, Some(9), "the time stays");
        assert_eq!(s.item("r1").unwrap().y, 50.0);
        s.undo();
        assert_eq!(
            (s.ink().get(a).unwrap().points[0].x, s.item("r1").unwrap().y),
            (0.0, 100.0)
        );
        s.redo();
        let (gone, area) = s.delete(&refs);
        assert_eq!(gone, refs);
        assert!(area.is_some() && s.is_empty() && s.order().is_empty());
        s.undo();
        assert_eq!(order(&s), ["s:0", "i:r1"]);
        assert_eq!(s.delete(&[Ref::Stroke(77)]), (vec![], None));
    }

    #[test]
    fn a_bound_arrow_moves_with_its_shape_and_its_free_end_with_it() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        let mut arrow = Item::new("ar", "arrow", Style::default());
        arrow.start = Some(End {
            x: 50.0,
            y: 25.0,
            bind: Some(Ref::Item("a".into())),
        });
        arrow.end = Some(End {
            x: 400.0,
            y: 25.0,
            bind: None,
        });
        with_item(&mut s, arrow);
        let (st, en) = s.ends(s.item("ar").unwrap()).unwrap();
        assert_eq!((st, en), (Pt::new(100.0, 25.0), Pt::new(400.0, 25.0)));
        s.move_refs(&[Ref::Item("a".into())], 0.0, 300.0);
        let (st, _) = s.ends(s.item("ar").unwrap()).unwrap();
        assert!(
            st.y >= 300.0,
            "re-routed without an edit of its own: {st:?}"
        );
        assert!(s.item_bounds(s.item("ar").unwrap()).is_some());
        // A move of both together shifts only the free end of the arrow.
        s.move_refs(&[Ref::Item("a".into()), Ref::Item("ar".into())], 10.0, 0.0);
        assert_eq!(s.item("ar").unwrap().end.as_ref().unwrap().x, 410.0);
        assert_eq!(
            s.shape_of(&Ref::Item("ar".into())),
            None,
            "never onto a line"
        );
    }

    #[test]
    fn erasing_and_the_lasso_take_strokes() {
        let mut s = empty();
        pen(&mut s, &[(0.0, 0.0), (100.0, 0.0)], None);
        pen(&mut s, &[(0.0, 50.0), (100.0, 50.0)], None);
        let (gone, _) = s.erase(&[(40.0, -30.0), (40.0, 80.0)], 4.0);
        assert_eq!(gone, vec![Ref::Stroke(0), Ref::Stroke(1)]);
        s.undo();
        let lassoed = s.lasso(&[(-10.0, -10.0), (200.0, -10.0), (200.0, 10.0), (-10.0, 10.0)]);
        assert_eq!(lassoed, vec![Ref::Stroke(0)]);
        assert_eq!(s.bounds_of(&lassoed).unwrap().x1, 101.0);
        assert!(s.bounds().is_some());
    }

    /// F21-AC3: 2 items and 3 strokes under one sweep go as one step; Undo puts all five
    /// back in their paint order, and the arrow bound to the erased rectangle is frozen at its
    /// last centre, then bound again by the Undo.
    #[test]
    fn one_sweep_takes_items_and_strokes_and_frees_bound_arrows() {
        let mut s = empty();
        pen(&mut s, &[(0.0, 0.0), (0.0, 100.0)], Some(1));
        let mut a = rect("a", 50.0, 0.0);
        a.style = Style::default().with("fill", "solid");
        with_item(&mut s, a);
        pen(&mut s, &[(200.0, 0.0), (200.0, 100.0)], Some(2));
        with_item(&mut s, rect("b", 250.0, 0.0));
        pen(&mut s, &[(400.0, 0.0), (400.0, 100.0)], None);
        let mut ar = Item::new("ar", "arrow", Style::default());
        ar.start = Some(End {
            x: 0.0,
            y: 0.0,
            bind: Some(Ref::Item("a".into())),
        });
        ar.end = Some(End {
            x: 900.0,
            y: 900.0,
            bind: None,
        });
        with_item(&mut s, ar);
        let before: Vec<String> = order(&s);
        // Across the strokes, through a's inside (filled) and b's left edge.
        let (gone, area) = s.erase(&[(-10.0, 30.0), (410.0, 30.0)], 3.0);
        assert_eq!(gone.len(), 5, "{gone:?}");
        assert!(area.is_some());
        let freed = s.item("ar").unwrap().start.clone().unwrap();
        assert_eq!(
            (freed.x, freed.y, freed.bind),
            (100.0, 25.0, None),
            "frozen at a's centre"
        );
        assert_eq!(order(&s), ["i:ar"]);
        s.undo();
        assert_eq!(order(&s), before);
        let back = s.item("ar").unwrap().start.clone().unwrap();
        assert_eq!(back.bind, Some(Ref::Item("a".into())), "bound again");
        // An unfilled shape's inside is not it.
        let (gone, _) = s.erase(&[(300.0, 25.0)], 3.0);
        assert!(gone.is_empty());
    }

    #[test]
    fn a_group_member_erases_its_whole_group() {
        let mut s = empty();
        let st = pen(&mut s, &[(0.0, 0.0), (10.0, 0.0)], None);
        let mut far = rect("far", 5000.0, 5000.0);
        far.group_id = Some("g".into());
        with_item(&mut s, far);
        s.doc.extras.insert(
            st.0,
            StrokeExtra {
                group_id: Some("g".into()),
                ..StrokeExtra::default()
            },
        );
        assert_eq!(s.group_of(&Ref::Stroke(st.0)), Some("g"));
        let (gone, _) = s.erase(&[(5.0, 0.0)], 3.0);
        assert_eq!(gone, vec![Ref::Stroke(0), Ref::Item("far".into())]);
        assert!(s.is_empty());
    }

    #[test]
    fn clear_is_one_step_and_a_backup_can_be_absorbed() {
        let mut s = empty();
        assert_eq!(s.clear(), None, "nothing to clear");
        pen(&mut s, &[(0.0, 0.0)], Some(5));
        with_item(&mut s, rect("a", 0.0, 0.0));
        let kept = s.clone();
        assert!(s.clear().is_some());
        assert!(s.is_empty());
        s.undo();
        assert_eq!(s.doc(), kept.doc());
        s.redo();
        // Absorbing the backup gives fresh ids where they are taken.
        pen(&mut s, &[(9.0, 9.0)], None);
        with_item(&mut s, rect("a", 1.0, 1.0));
        assert!(s.absorb(&kept).is_some());
        assert_eq!(s.ink().strokes().len(), 2);
        assert_eq!(s.doc().items.len(), 2);
        assert_ne!(s.doc().items[1].id, "a");
        assert_eq!(s.ink().strokes()[1].rec_ms, Some(5), "times come back");
        assert_ne!(s.ink().strokes()[1].id, s.ink().strokes()[0].id);
        s.undo();
        assert_eq!(s.ink().strokes().len(), 1);
        assert_eq!(s.absorb(&empty()), None);
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        pen(&mut broken, &[(0.0, 0.0)], None);
        assert_eq!(broken.clear(), None);
        assert_eq!(broken.absorb(&kept), None);
    }

    fn bound_arrow(id: &str, from: &str, to: Option<&str>) -> Item {
        let mut ar = Item::new(id, "arrow", Style::default());
        ar.start = Some(End {
            x: 0.0,
            y: 0.0,
            bind: Some(Ref::Item(from.into())),
        });
        ar.end = Some(End {
            x: 800.0,
            y: 25.0,
            bind: to.map(|t| Ref::Item(t.into())),
        });
        ar
    }

    /// F21-FR4 / AC7 (selection part): a pen rectangle touching 2 shapes and 1 stroke selects
    /// all 3; a tap takes the topmost; the lasso takes enclosed items too.
    #[test]
    fn selecting_by_rectangle_tap_and_loop() {
        let mut s = empty();
        with_item(&mut s, rect("a", 0.0, 0.0));
        with_item(&mut s, rect("b", 200.0, 0.0));
        pen(&mut s, &[(0.0, 300.0), (50.0, 300.0)], None);
        with_item(&mut s, rect("far", 5000.0, 0.0));
        let r = BBox {
            x0: 50.0,
            y0: 40.0,
            x1: 210.0,
            y1: 310.0,
        };
        assert_eq!(
            s.in_rect(r),
            vec![Ref::Item("a".into()), Ref::Item("b".into()), Ref::Stroke(0)]
        );
        with_item(&mut s, rect("top", 20.0, 10.0));
        assert_eq!(
            s.top_at(Pt::new(50.0, 25.0), 4.0),
            Some(Ref::Item("top".into()))
        );
        assert_eq!(s.top_at(Pt::new(25.0, 300.0), 4.0), Some(Ref::Stroke(0)));
        assert_eq!(s.top_at(Pt::new(-500.0, 0.0), 4.0), None);
        let lasso = s.lasso(&[(-10.0, -10.0), (110.0, -10.0), (110.0, 60.0), (-10.0, 60.0)]);
        assert_eq!(
            lasso,
            vec![Ref::Item("a".into())],
            "top sticks out of the loop"
        );
        with_item(&mut s, bound_arrow("ar", "a", None));
        assert!(
            !s.lasso(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)])
                .contains(&Ref::Item("ar".into()))
        );
        s.select(&[Ref::Item("a".into()), Ref::Item("gone".into())]);
        assert_eq!(s.selection, vec![Ref::Item("a".into())]);
    }

    /// F21-FR4: duplicates land 24 units down and right with new ids and groups, untimed; an
    /// arrow keeps only bindings to shapes copied with it.
    #[test]
    fn duplicates_are_new_untimed_and_keep_inner_bindings() {
        let mut s = empty();
        let mut a = rect("a", 0.0, 0.0);
        a.rec_ms = Some(5);
        a.group_id = Some("g".into());
        with_item(&mut s, a);
        with_item(&mut s, rect("b", 400.0, 0.0));
        with_item(&mut s, bound_arrow("ar", "a", Some("b")));
        let st = pen(&mut s, &[(0.0, 100.0)], Some(7));
        s.doc.extras.insert(
            st.0,
            StrokeExtra {
                group_id: Some("g".into()),
                ..StrokeExtra::default()
            },
        );
        let area = s.duplicate(
            &[Ref::Item("a".into()), Ref::Item("ar".into())],
            "2026-10-10T09:00:00+00:00",
        );
        assert!(area.is_some());
        assert_eq!(s.selection.len(), 3, "a, its group's stroke and the arrow");
        let ca = s.item(&s.selection[0].to_string()[2..]).unwrap().clone();
        assert_eq!((ca.x, ca.y, ca.rec_ms), (24.0, 24.0, None));
        assert!(ca.group_id.is_some() && ca.group_id.as_deref() != Some("g"));
        assert_eq!(ca.created_at.as_deref(), Some("2026-10-10T09:00:00+00:00"));
        let car = s.item(&s.selection[1].to_string()[2..]).unwrap().clone();
        assert_eq!(
            car.start.as_ref().unwrap().bind,
            Some(Ref::Item(ca.id.clone()))
        );
        let end = car.end.as_ref().unwrap();
        assert_eq!(
            (end.bind.clone(), end.x),
            (None, 424.0),
            "frozen where b's edge was"
        );
        let cs = s.selection[2].stroke().unwrap();
        assert_eq!(s.ink().get(cs).unwrap().rec_ms, None);
        assert_eq!(s.ink().get(cs).unwrap().points[0].x, 24.0);
        assert_eq!(s.doc().extras[&cs.0].group_id, ca.group_id);
        s.undo();
        assert_eq!(s.doc().items.len(), 3);
        assert_eq!(s.duplicate(&[], "t"), None);
        let (mut broken, _) = Scene::load(InkLayer::new(), Err("io".into()));
        assert_eq!(broken.duplicate(&[Ref::Stroke(0)], "t"), None);
    }

    #[test]
    fn loading_drops_stale_and_bad_entries() {
        let mut ink = InkLayer::new();
        for x in [0.0, 10.0, 20.0] {
            let p = vec![InkPoint::new(x, 0.0, 1.0, None, None, 0).unwrap()];
            ink.add(Tool::Pen, InkColor::BLACK, 2.0, p, 1, None)
                .unwrap();
        }
        let file = json!({"version": 1, "order": ["s:2", "s:7", "i:a", "bogus", "s:2", "i:gone"],
            "items": [rect("a", 0.0, 0.0).to_value(), {"id": "../x", "type": "rectangle"},
                      rect("a", 5.0, 5.0).to_value()],
            "strokes": {"0": {"groupId": "g-1", "link": null}, "7": {"groupId": "g-1"}, "x": {}, "1": {}},
            "defaultStyleAtSave": {"color": "blue"}, "later": true});
        let (s, r) = Scene::load(ink, Ok(Some(serde_json::to_vec(&file).unwrap())));
        assert_eq!(
            r,
            LoadReport {
                dropped: 4,
                stale: 2
            }
        );
        assert_eq!(order(&s), ["s:2", "i:a", "s:0", "s:1"]);
        assert_eq!(s.doc().extras.len(), 1, "an empty entry needs no place");
        let saved: Value = serde_json::from_slice(&s.items_json()).unwrap();
        assert_eq!(saved["later"], true);
        assert_eq!(saved["defaultStyleAtSave"]["color"], "blue");
        assert!(!saved.to_string().contains("s:7"));
        assert!(s.items_to_save().is_some(), "it changed on load");
    }

    #[test]
    fn an_unreadable_items_file_is_never_written_and_items_are_left_alone() {
        for items in [
            Ok(Some(b"{not json".to_vec())),
            Ok(Some(b"{\"version\": 9}".to_vec())),
            Err("io".to_owned()),
        ] {
            let (mut s, _) = Scene::load(InkLayer::new(), items);
            assert!(!s.items_ok());
            pen(&mut s, &[(0.0, 0.0)], None);
            with_item(&mut s, rect("r", 0.0, 0.0));
            assert_eq!(s.items_to_save(), None);
            assert_eq!(s.delete(&[Ref::Item("r".into())]).0, vec![]);
            assert!(s.move_refs(&[Ref::Item("r".into())], 1.0, 1.0).is_none());
            assert!(s.dirty(), "the strokes still save");
        }
    }

    #[test]
    fn items_json_is_written_only_when_needed() {
        let mut s = empty();
        assert_eq!(s.items_to_save(), None);
        pen(&mut s, &[(0.0, 0.0)], None);
        assert_eq!(s.items_to_save(), None, "strokes alone need no items.json");
        assert!(s.dirty() && s.strokes_dirty());
        s.strokes_saved();
        assert!(!s.dirty());
        with_item(&mut s, rect("r", 0.0, 0.0));
        let b = s.items_to_save().unwrap();
        s.items_saved(b);
        assert_eq!(s.items_to_save(), None);
        pen(&mut s, &[(5.0, 5.0)], None);
        assert!(s.items_to_save().is_some(), "the order grew");
    }

    #[test]
    fn a_binding_to_something_gone_is_dropped_on_load() {
        let mut arrow = Item::new("ar", "arrow", Style::default());
        arrow.start = Some(End {
            x: 1.0,
            y: 2.0,
            bind: Some(Ref::Stroke(7)),
        });
        arrow.end = Some(End {
            x: 3.0,
            y: 4.0,
            bind: Some(Ref::Item("ar".into())),
        });
        arrow.link = Link::meeting("meeting-20261008-090000");
        let file = json!({"version": 1, "items": [arrow.to_value()]});
        let (s, r) = Scene::load(
            InkLayer::new(),
            Ok(Some(serde_json::to_vec(&file).unwrap())),
        );
        assert_eq!(r.stale, 2);
        let it = s.item("ar").unwrap();
        assert_eq!(it.start.as_ref().unwrap().bind, None);
        assert_eq!(it.end.as_ref().unwrap().bind, None);
        assert!(it.link.is_some());
        assert!(s.exists(&Ref::Item("ar".into())) && !s.exists(&Ref::Stroke(0)));
    }
}
