//! F17-FR4 / FR5: the notes canvas behind JNI. Each open layer is held in memory while its
//! page is open; a stroke changes it there and marks it dirty, and the shell flushes it on a
//! short deferred timer and when the page goes (`nativeInkSave`, as inkread's 1 500 ms flush,
//! `StylusInkController.kt:87-94`). Coordinates are world units (R20); times recording-clock ms.
use super::App;
use crate::api::error_json;
use crate::canvas::draw::{bbox_json, refs_json, stroke_json};
use crate::canvas::{Ref, Scene};
use crate::ink::{BBox, InkColor, InkLayer, InkPoint, Tool};
use crate::notes::{self, Layer, View};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// One open ink layer: where it is saved, its scene (F21-FR1: strokes and items with one
/// undo history; the Minutes layer has strokes only), and whether it changed since.
#[derive(Debug)]
pub(crate) struct Open {
    pub(crate) dir: PathBuf,
    pub(crate) scene: Scene,
    pub(crate) dirty: bool,
}

/// The ink layers open now, by meeting and layer.
#[derive(Debug, Default)]
pub struct OpenInk(Mutex<HashMap<(String, Layer), Open>>);

impl OpenInk {
    fn lock(&self) -> MutexGuard<'_, HashMap<(String, Layer), Open>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The highlighter's colour: a light band under the writing (inkread's default shade).
const HIGHLIGHT: InkColor = InkColor {
    r: 0,
    g: 0,
    b: 0,
    a: 64,
};

/// `[x, y, pressure, t]` per point (F17-FR4) as ink points, the stroke's recording time
/// (its first point's `t`; negative is untimed) and nothing else. `None` when malformed.
fn points_of(xypt: &[f32]) -> Option<(Vec<InkPoint>, Option<u64>)> {
    if xypt.is_empty() || !xypt.len().is_multiple_of(4) {
        return None;
    }
    let t0 = xypt[3];
    let rec = (t0.is_finite() && t0 >= 0.0).then_some(t0 as u64);
    let pts = xypt
        .chunks_exact(4)
        .map(|q| {
            let dt = match rec {
                Some(_) if q[3].is_finite() => (q[3] - t0).clamp(0.0, u32::MAX as f32) as u32,
                _ => 0,
            };
            InkPoint::new(q[0], q[1], q[2], None, None, dt).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some((pts, rec))
}

/// Interleaved `[x, y, …]` as pairs; `None` when odd or not finite.
fn pairs(xy: &[f32]) -> Option<Vec<(f32, f32)>> {
    if !xy.len().is_multiple_of(2) || xy.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(xy.chunks_exact(2).map(|p| (p[0], p[1])).collect())
}

/// A JSON list of stroke ids, as stroke refs.
fn ids_of(json: &str) -> Option<Vec<Ref>> {
    let v: Vec<u32> = serde_json::from_str(json).ok()?;
    Some(v.into_iter().map(Ref::Stroke).collect())
}

/// The stroke ids among `refs` (the F17 calls answer in stroke ids).
fn stroke_ids(refs: &[Ref]) -> Vec<u32> {
    refs.iter().filter_map(Ref::stroke).map(|s| s.0).collect()
}

impl App {
    /// Run `f` on `id`'s open `layer`, loading it first. A layer that can't be read is never
    /// opened, so nothing can save over it.
    pub(crate) fn with_ink<T>(
        &self,
        id: &str,
        layer: Layer,
        f: impl FnOnce(&mut Open) -> T,
    ) -> Result<T, String> {
        if !crate::sidecar::is_valid_id(id) {
            return Err("invalid meeting id".into());
        }
        let mut open = self.ink.lock();
        let key = (id.to_owned(), layer);
        if !open.contains_key(&key) {
            let dir = self.library().find(id).map_err(|e| e.to_string())?.dir;
            let scene = match layer {
                Layer::Canvas => notes::load_scene(&dir, id, 1)?.0,
                Layer::Minutes => Scene::load(notes::load(&dir, id, layer)?, Ok(None)).0,
            };
            open.insert(
                key.clone(),
                Open {
                    dir,
                    scene,
                    dirty: false,
                },
            );
        }
        let o = open.get_mut(&key).expect("inserted above");
        Ok(f(o))
    }

    /// `nativeNotesOpen(id)`: opens the canvas and replies `{view, clock, bounds, strokes,
    /// items, imagesDir}`, with the last viewport (`null`: fit the content), the content's
    /// bounds (`null`: none yet) and where image items' copies are (F21-FR11).
    pub fn notes_open_json(&self, id: &str) -> String {
        let r = self.with_ink(id, Layer::Canvas, |o| {
            let images = notes::folder(&o.dir, id).join(notes::IMAGES);
            json!({"view": notes::read_view(&o.dir, id), "clock": "recording-ms",
                   "bounds": bbox_json(o.scene.bounds()), "strokes": o.scene.ink().strokes().len(),
                   "items": o.scene.doc().items.len(), "imagesDir": images.to_string_lossy()})
        });
        r.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkStroke`: one whole stroke, batched `[x, y, pressure, t]` in world units with
    /// `t` on the recording clock (negative: untimed). The first stroke names the notes in the
    /// sidecar. `{strokeId, bbox, atMs}`.
    pub fn ink_stroke_json(
        &self,
        id: &str,
        layer: &str,
        tool: i32,
        width: f32,
        xypt: &[f32],
    ) -> String {
        self.stroke_json(id, layer, tool, width, None, xypt)
    }

    /// `nativeInkStrokeStyled(id, tool, styleJson, xypt)` (F21-FR7): a canvas stroke drawn with
    /// a style: the Pen's colour (true RGBA in `.inkbin`), opacity and size (its nominal
    /// width), its style ids in `items.json`. The Highlighter is unchanged.
    pub fn ink_stroke_styled_json(&self, id: &str, tool: i32, style: &str, xypt: &[f32]) -> String {
        let Ok(v) = serde_json::from_str::<Value>(style) else {
            return error_json("the style must be JSON");
        };
        let st = crate::canvas::Style::from_value(&v);
        let width = st.stroke_width();
        self.stroke_json(id, "canvas", tool, width, Some(st), xypt)
    }

    fn stroke_json(
        &self,
        id: &str,
        layer: &str,
        tool: i32,
        width: f32,
        style: Option<crate::canvas::Style>,
        xypt: &[f32],
    ) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let Some(tool) = u8::try_from(tool).ok().and_then(Tool::from_code) else {
            return error_json("unknown tool");
        };
        let Some((pts, rec)) = points_of(xypt) else {
            return error_json("points must be [x, y, pressure, t] quadruples");
        };
        let style = style.filter(|_| tool == Tool::Pen);
        let color = if tool == Tool::Highlighter {
            HIGHLIGHT
        } else {
            style
                .as_ref()
                .map_or(InkColor::BLACK, crate::canvas::edits::ink_color)
        };
        let now = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0);
        let r = self.with_ink(id, layer, |o| {
            let st = o
                .scene
                .new_stroke(tool, color, width, pts, now, rec)
                .map_err(|e| e.to_string())?;
            let bbox = st.bounds();
            let sid = o.scene.add_stroke(st, style);
            o.dirty = true;
            Ok::<_, String>((sid, Some(bbox)))
        });
        let (sid, bbox) = match r.and_then(|x| x) {
            Ok(v) => v,
            Err(e) => return error_json(&e),
        };
        if let Err(e) = self.declare_notes(id) {
            return error_json(&e);
        }
        json!({"strokeId": sid.0, "bbox": bbox_json(bbox), "atMs": rec}).to_string()
    }

    /// Name the notes folder in the sidecar on the first stroke (Data Model `notes`).
    pub(crate) fn declare_notes(&self, id: &str) -> Result<(), String> {
        let _w = self.write_lock();
        let lib = self.library();
        let has = lib
            .load_for_write(id)
            .map(|(_, sc)| notes::has_notes(&sc))
            .map_err(|e| e.to_string())?;
        if has {
            return Ok(());
        }
        lib.update(id, |sc, _| {
            notes::declare(sc);
            Ok(())
        })
        .map_err(|e| e.to_string())
    }

    /// `nativeInkErase`: an eraser sweep through `xy` (world) with radius `r`, one undo step.
    /// `{erased: [id], bbox}`.
    pub fn ink_erase_json(&self, id: &str, layer: &str, xy: &[f32], r: f32) -> String {
        let (Some(layer), Some(path)) = (Layer::from_id(layer), pairs(xy)) else {
            return error_json("an eraser path is [x, y] pairs on a known layer");
        };
        let out = self.with_ink(id, layer, |o| {
            let (gone, bbox) = o.scene.erase(&path, r);
            o.dirty |= !gone.is_empty();
            json!({"erased": stroke_ids(&gone), "refs": refs_json(&gone), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkUndo` / `nativeInkRedo`: `{changed, bbox, canUndo, canRedo}`. On the canvas
    /// they are scene-wide: strokes and items share one history (F21-FR1).
    pub fn ink_history_json(&self, id: &str, layer: &str, redo: bool) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let out = self.with_ink(id, layer, |o| {
            let r = if redo { o.scene.redo() } else { o.scene.undo() };
            o.dirty |= r.is_some();
            json!({"changed": r.is_some(), "bbox": bbox_json(r.flatten()),
                   "canUndo": o.scene.can_undo(), "canRedo": o.scene.can_redo()})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkLasso`: the strokes a loop (world `[x, y, …]`) takes, `{ids, refs, bbox}`;
    /// they become the selection.
    pub fn ink_lasso_json(&self, id: &str, layer: &str, xy: &[f32]) -> String {
        let (Some(layer), Some(poly)) = (Layer::from_id(layer), pairs(xy)) else {
            return error_json("a lasso is [x, y] pairs on a known layer");
        };
        let out = self.with_ink(id, layer, |o| {
            let refs = o.scene.lasso(&poly);
            let bbox = o.scene.bounds_of(&refs);
            o.scene.selection.clone_from(&refs);
            json!({"ids": stroke_ids(&refs), "refs": refs_json(&refs), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkMove`: moves a lasso selection (`[id]` JSON) by `(dx, dy)` world units, one
    /// undo step. `{moved, bbox}`, the area before and after.
    pub fn ink_move_json(&self, id: &str, layer: &str, ids: &str, dx: f32, dy: f32) -> String {
        let (Some(layer), Some(refs)) = (Layer::from_id(layer), ids_of(ids)) else {
            return error_json("a move needs a known layer and a list of stroke ids");
        };
        let out = self.with_ink(id, layer, |o| {
            let area = o.scene.move_refs(&refs, f64::from(dx), f64::from(dy));
            o.dirty |= area.is_some();
            json!({"moved": area.is_some(), "bbox": bbox_json(area)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkDelete`: deletes a lasso selection, one undo step. `{deleted: [id], bbox}`.
    pub fn ink_delete_json(&self, id: &str, layer: &str, ids: &str) -> String {
        let (Some(layer), Some(refs)) = (Layer::from_id(layer), ids_of(ids)) else {
            return error_json("a delete needs a known layer and a list of stroke ids");
        };
        let out = self.with_ink(id, layer, |o| {
            let (gone, bbox) = o.scene.delete(&refs);
            o.dirty |= !gone.is_empty();
            json!({"deleted": stroke_ids(&gone), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkStrokesForDraw`: the strokes meeting the world rectangle, in paint order.
    pub fn ink_strokes_json(&self, id: &str, layer: &str, rect: [f32; 4]) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let r = rect_of(rect);
        let out = self.with_ink(id, layer, |o| {
            let s = &o.scene;
            let list: Vec<Value> = s
                .order()
                .iter()
                .filter_map(|r| r.stroke())
                .filter_map(|sid| s.ink().get(sid))
                .filter(|st| st.bounds().meets(r))
                .map(|st| stroke_json(st, s.doc().extras.get(&st.id.0)))
                .collect();
            json!({"strokes": list, "bounds": bbox_json(s.bounds()),
                   "canUndo": s.can_undo(), "canRedo": s.can_redo()})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkHit(id, layer, x, y, r)` (F17-FR7, Listen): the topmost stroke within `r` of the
    /// world point. A timed one gives where playback starts (`max(0, t − 2 s)`, placed in its
    /// segment) and the transcript line spoken at `t`: `{strokeId, atMs, seekMs, segIndex,
    /// segOffsetMs, lineN}`; an untimed one `{strokeId, atMs: null}`; none `{strokeId: null}`.
    pub fn ink_hit_json(&self, id: &str, layer: &str, x: f32, y: f32, r: f32) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let hit = match self.with_ink(id, layer, |o| {
            o.scene.ink().top_hit(x, y, r).map(|s| (s.id.0, s.rec_ms))
        }) {
            Ok(h) => h,
            Err(e) => return error_json(&e),
        };
        let Some((sid, rec)) = hit else {
            return json!({"strokeId": null}).to_string();
        };
        let Some(t) = rec else {
            return json!({"strokeId": sid, "atMs": null}).to_string();
        };
        let sc = match self.library().get(id) {
            Ok(v) => v.meeting,
            Err(e) => return error_json(&e.to_string()),
        };
        let s = notes::seek(&sc, t);
        let line = super::live::line_at(&super::live::transcript_rows(&sc), t);
        json!({"strokeId": sid, "atMs": t, "seekMs": s.seek_ms, "segIndex": s.seg_index,
               "segOffsetMs": s.seg_offset_ms, "lineN": line})
        .to_string()
    }

    /// Write every changed layer of `id` (or of every meeting when `None`). `(saved, error)`.
    pub(crate) fn flush_ink(&self, id: Option<&str>) -> (usize, Option<String>) {
        let mut open = self.ink.lock();
        let mut saved = 0;
        let mut error = None;
        for ((mid, layer), o) in open.iter_mut() {
            if !o.dirty || id.is_some_and(|i| i != mid) {
                continue;
            }
            let r = match layer {
                Layer::Canvas => notes::save_scene(&o.dir, mid, 1, &mut o.scene),
                Layer::Minutes => {
                    let ink = InkLayer::from_strokes(o.scene.ink().strokes().to_vec());
                    notes::save(&o.dir, mid, *layer, &ink).map(|()| true)
                }
            };
            match r {
                Ok(wrote) => {
                    o.dirty = false;
                    saved += usize::from(wrote);
                }
                Err(e) => error = Some(format!("The notes can't be saved ({:?})", e.kind())),
            }
        }
        (saved, error)
    }

    /// `nativeEditMinutes(id, patchJson)` (F17-FR10): one correction to the minutes, marking
    /// that field the user's. `{meeting}`.
    pub fn edit_minutes_json(&self, id: &str, patch: &str) -> String {
        let Ok(patch) = serde_json::from_str::<Value>(patch) else {
            return error_json("the correction must be JSON");
        };
        let _w = self.write_lock();
        let r = self.library().update(id, |sc, _| {
            crate::minutes::edit(sc, &patch).map_err(crate::library::LibError::Invalid)?;
            Ok(sc.clone())
        });
        match r {
            Ok(sc) => json!({"meeting": sc}).to_string(),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// `nativeInkSave(id)`: the deferred flush (F17-FR5). `{saved}`, or `{error}`.
    pub fn ink_save_json(&self, id: &str) -> String {
        match self.flush_ink(Some(id)) {
            (_, Some(e)) => error_json(&e),
            (n, None) => json!({"saved": n}).to_string(),
        }
    }

    /// `nativeNotesClose(id, cx, cy, zoom)`: the page goes. The ink is saved, the viewport
    /// kept for next time and the layers let go. `{saved}`.
    pub fn notes_close_json(&self, id: &str, cx: f64, cy: f64, zoom: f64) -> String {
        let (n, err) = self.flush_ink(Some(id));
        if let Some(e) = err {
            return error_json(&e);
        }
        let mut open = self.ink.lock();
        if let Some(o) = open.get(&(id.to_owned(), Layer::Canvas)) {
            self.clean_images(id, &o.dir, &o.scene);
        }
        let dir = open
            .get(&(id.to_owned(), Layer::Canvas))
            .map(|o| o.dir.clone());
        open.retain(|(mid, _), _| mid != id);
        drop(open);
        if let Some(dir) = dir {
            let _ = notes::write_view(&dir, id, View { cx, cy, zoom });
        }
        json!({"saved": n}).to_string()
    }
}

/// A world rectangle from `[x0, y0, x1, y1]`.
pub(crate) fn rect_of(r: [f32; 4]) -> BBox {
    BBox {
        x0: r[0],
        y0: r[1],
        x1: r[2],
        y1: r[3],
    }
}
