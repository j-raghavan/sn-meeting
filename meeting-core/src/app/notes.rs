//! F17-FR4 / FR5: the notes canvas behind JNI. Each open layer is held in memory while its
//! page is open; a stroke changes it there and marks it dirty, and the shell flushes it on a
//! short deferred timer and when the page goes (`nativeInkSave`, as inkread's 1 500 ms flush,
//! `StylusInkController.kt:87-94`). Coordinates are world units (R20); times recording-clock ms.
use super::App;
use crate::api::error_json;
use crate::ink::{BBox, InkColor, InkLayer, InkPoint, Stroke, StrokeId, Tool};
use crate::notes::{self, Layer, View};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// One open ink layer: where it is saved, the ink, and whether it changed since.
#[derive(Debug)]
struct Open {
    dir: PathBuf,
    ink: InkLayer,
    dirty: bool,
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

/// `[x0, y0, x1, y1]`, or `null`.
fn bbox_json(b: Option<BBox>) -> Value {
    b.map_or(Value::Null, |b| json!([b.x0, b.y0, b.x1, b.y1]))
}

/// A stroke as the view draws it: `{id, tool, width, color, atMs, pts: [x, y, …]}`.
fn stroke_json(s: &Stroke) -> Value {
    let pts: Vec<f32> = s.points.iter().flat_map(|p| [p.x, p.y]).collect();
    json!({"id": s.id.0, "tool": s.tool.code(), "width": s.width,
           "color": [s.color.r, s.color.g, s.color.b, s.color.a], "atMs": s.rec_ms, "pts": pts})
}

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

/// A JSON list of stroke ids.
fn ids_of(json: &str) -> Option<Vec<StrokeId>> {
    let v: Vec<u32> = serde_json::from_str(json).ok()?;
    Some(v.into_iter().map(StrokeId).collect())
}

impl App {
    /// Run `f` on `id`'s open `layer`, loading it first. A layer that can't be read is never
    /// opened, so nothing can save over it.
    fn with_ink<T>(
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
            let ink = notes::load(&dir, id, layer)?;
            open.insert(
                key.clone(),
                Open {
                    dir,
                    ink,
                    dirty: false,
                },
            );
        }
        let o = open.get_mut(&key).expect("inserted above");
        Ok(f(o))
    }

    /// `nativeNotesOpen(id)`: opens the canvas and replies `{view, clock, bounds, strokes}`,
    /// with the last viewport (`null`: fit the ink) and the ink's bounds (`null`: none yet).
    pub fn notes_open_json(&self, id: &str) -> String {
        let r = self.with_ink(id, Layer::Canvas, |o| {
            json!({"view": notes::read_view(&o.dir, id), "clock": "recording-ms",
                   "bounds": bbox_json(o.ink.bounds()), "strokes": o.ink.strokes().len()})
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
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let Some(tool) = u8::try_from(tool).ok().and_then(Tool::from_code) else {
            return error_json("unknown tool");
        };
        let Some((pts, rec)) = points_of(xypt) else {
            return error_json("points must be [x, y, pressure, t] quadruples");
        };
        let color = if tool == Tool::Highlighter {
            HIGHLIGHT
        } else {
            InkColor::BLACK
        };
        let now = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0);
        let r = self.with_ink(id, layer, |o| {
            let id = o
                .ink
                .add(tool, color, width, pts, now, rec)
                .map_err(|e| e.to_string())?;
            o.dirty = true;
            Ok::<_, String>((id, o.ink.get(id).map(Stroke::bounds)))
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
    fn declare_notes(&self, id: &str) -> Result<(), String> {
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
            let hit = o.ink.hit_by_path(&path, r);
            let bbox = o.ink.bounds_of(&hit);
            let gone = o.ink.delete_strokes(&hit);
            o.dirty |= !gone.is_empty();
            json!({"erased": gone.iter().map(|s| s.0).collect::<Vec<_>>(), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkUndo` / `nativeInkRedo`: `{changed, bbox, canUndo, canRedo}`.
    pub fn ink_history_json(&self, id: &str, layer: &str, redo: bool) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let out = self.with_ink(id, layer, |o| {
            let r = if redo { o.ink.redo() } else { o.ink.undo() };
            o.dirty |= r.is_some();
            json!({"changed": r.is_some(), "bbox": bbox_json(r.flatten()),
                   "canUndo": o.ink.can_undo(), "canRedo": o.ink.can_redo()})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkLasso`: the strokes a loop (world `[x, y, …]`) takes, `{ids, bbox}`.
    pub fn ink_lasso_json(&self, id: &str, layer: &str, xy: &[f32]) -> String {
        let (Some(layer), Some(poly)) = (Layer::from_id(layer), pairs(xy)) else {
            return error_json("a lasso is [x, y] pairs on a known layer");
        };
        let out = self.with_ink(id, layer, |o| {
            let ids = crate::ink::select_in_polygon(&o.ink, &poly);
            let bbox = crate::ink::selection_bounds(&o.ink, &ids);
            json!({"ids": ids.iter().map(|s| s.0).collect::<Vec<_>>(), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkMove`: moves a lasso selection (`[id]` JSON) by `(dx, dy)` world units, one
    /// undo step. `{moved, bbox}`, the area before and after.
    pub fn ink_move_json(&self, id: &str, layer: &str, ids: &str, dx: f32, dy: f32) -> String {
        let (Some(layer), Some(ids)) = (Layer::from_id(layer), ids_of(ids)) else {
            return error_json("a move needs a known layer and a list of stroke ids");
        };
        let out = self.with_ink(id, layer, |o| {
            let before = o.ink.bounds_of(&ids);
            let moved = o.ink.move_strokes(&ids, dx, dy);
            o.dirty |= moved;
            let after = o.ink.bounds_of(&ids);
            let area = before.zip(after).map(|(a, b)| a.union(b));
            json!({"moved": moved, "bbox": bbox_json(area)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkDelete`: deletes a lasso selection, one undo step. `{deleted: [id], bbox}`.
    pub fn ink_delete_json(&self, id: &str, layer: &str, ids: &str) -> String {
        let (Some(layer), Some(ids)) = (Layer::from_id(layer), ids_of(ids)) else {
            return error_json("a delete needs a known layer and a list of stroke ids");
        };
        let out = self.with_ink(id, layer, |o| {
            let bbox = o.ink.bounds_of(&ids);
            let gone = o.ink.delete_strokes(&ids);
            o.dirty |= !gone.is_empty();
            json!({"deleted": gone.iter().map(|s| s.0).collect::<Vec<_>>(), "bbox": bbox_json(bbox)})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeInkStrokesForDraw`: the strokes meeting the world rectangle, in paint order.
    pub fn ink_strokes_json(&self, id: &str, layer: &str, rect: [f32; 4]) -> String {
        let Some(layer) = Layer::from_id(layer) else {
            return error_json("unknown layer");
        };
        let r = BBox {
            x0: rect[0],
            y0: rect[1],
            x1: rect[2],
            y1: rect[3],
        };
        let out = self.with_ink(id, layer, |o| {
            let list: Vec<Value> = o.ink.strokes_in(r).into_iter().map(stroke_json).collect();
            json!({"strokes": list, "bounds": bbox_json(o.ink.bounds()),
                   "canUndo": o.ink.can_undo(), "canRedo": o.ink.can_redo()})
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
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
            match notes::save(&o.dir, mid, *layer, &o.ink) {
                Ok(()) => {
                    o.dirty = false;
                    saved += 1;
                }
                Err(e) => error = Some(format!("The notes can't be saved ({:?})", e.kind())),
            }
        }
        (saved, error)
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
