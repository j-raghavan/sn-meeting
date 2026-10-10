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
use std::collections::{BTreeMap, HashSet};

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
    fn step(&mut self, f: impl FnOnce(&mut Scene) -> (Placed, Placed)) -> Option<BBox> {
        let before = self.doc.clone();
        let (removed, added) = f(self);
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
            let removed = s.ink.take(&ids);
            s.remove_from_doc(&refs);
            (removed, Vec::new())
        });
        (refs, area)
    }

    /// Take `refs` out of the item side: items, extras and order entries.
    fn remove_from_doc(&mut self, refs: &[Ref]) {
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

    /// Replace each stroke in `refs` by `f` of it, keeping its id and index: the lists a
    /// step records.
    fn map_strokes(&mut self, refs: &[Ref], f: impl Fn(&mut Stroke)) -> (Placed, Placed) {
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

    /// An eraser sweep through `path` with radius `r`: every stroke it touches, as one step
    /// (F17; items join in F21-FR3). The refs erased, in paint order.
    pub fn erase(&mut self, path: &[(f32, f32)], radius: f32) -> (Vec<Ref>, Option<BBox>) {
        let hit: Vec<Ref> = self
            .ink
            .hit_by_path(path, radius)
            .into_iter()
            .map(|s| Ref::Stroke(s.0))
            .collect();
        self.delete(&hit)
    }

    /// The strokes a lasso loop takes (F17), as refs in paint order.
    #[must_use]
    pub fn lasso(&self, poly: &[(f32, f32)]) -> Vec<Ref> {
        let ids = crate::ink::select_in_polygon(&self.ink, poly);
        self.doc
            .order
            .iter()
            .filter(|r| r.stroke().is_some_and(|s| ids.contains(&s)))
            .cloned()
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
