//! F21 behind JNI: the canvas's scene (strokes and items, one history) as the shell draws and
//! edits it. The scene is the open canvas layer of `super::notes`; every edit is one step.
use super::App;
use super::notes::rect_of;
use crate::api::error_json;
use crate::canvas::draw::{bbox_json, refs_json};
use crate::canvas::edits::{Corner, Measured};
use crate::canvas::geom::Pt;
use crate::canvas::{Ref, Scene};
use crate::ink::BBox;
use crate::notes::{self, Layer};
use serde_json::{Value, json};

/// Local wall time for an item's `createdAt` (display only; F21-FR1).
pub(crate) fn now_local() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// A finite number field of an op.
fn num(op: &Value, k: &str) -> Result<f64, String> {
    op.get(k)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{k} must be a number"))
}

/// The op's one `ref`.
fn one(op: &Value) -> Result<Ref, String> {
    op.get("ref")
        .and_then(Value::as_str)
        .and_then(Ref::parse)
        .ok_or_else(|| "ref must be s:<id> or i:<id>".into())
}

/// The op's world point `{x, y}`.
fn at(op: &Value) -> Result<Pt, String> {
    Ok(Pt::new(num(op, "x")?, num(op, "y")?))
}

/// The op's `refs`, or the selection when it names none.
fn refs_of(op: &Value, scene: &Scene) -> Vec<Ref> {
    match op.get("refs").and_then(Value::as_array) {
        Some(a) => a
            .iter()
            .filter_map(Value::as_str)
            .filter_map(Ref::parse)
            .collect(),
        None => scene.selection.clone(),
    }
}

/// `n` finite numbers from an op's array field.
fn floats(op: &Value, k: &str) -> Option<Vec<f32>> {
    #[allow(clippy::cast_possible_truncation)]
    let v: Vec<f32> = op
        .get(k)?
        .as_array()?
        .iter()
        .map(|x| x.as_f64().filter(|f| f.is_finite()).map(|f| f as f32))
        .collect::<Option<_>>()?;
    Some(v)
}

/// F21-FR4: `select` (not an undo step): `{at: [x, y], r}` a pen tap (the topmost thing, or
/// nothing on empty canvas), `{rect: [x0, y0, x1, y1]}` a selection rectangle, `{polygon:
/// [x, y, …]}` the lasso, `{refs}` given refs; groups join whole.
fn select(scene: &mut Scene, op: &Value) -> Result<(), String> {
    let refs = if let Some(at) = floats(op, "at").filter(|a| a.len() == 2) {
        let r = op.get("r").and_then(Value::as_f64).unwrap_or(8.0);
        let p = Pt::new(f64::from(at[0]), f64::from(at[1]));
        scene.top_at(p, r).into_iter().collect()
    } else if let Some(b) = floats(op, "rect").filter(|a| a.len() == 4) {
        let rect = BBox {
            x0: b[0].min(b[2]),
            y0: b[1].min(b[3]),
            x1: b[0].max(b[2]),
            y1: b[1].max(b[3]),
        };
        scene.in_rect(rect)
    } else if let Some(p) = floats(op, "polygon").filter(|a| a.len() % 2 == 0) {
        let poly: Vec<(f32, f32)> = p.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        scene.lasso(&poly)
    } else if op.get("refs").is_some() {
        refs_of(op, scene)
    } else if op.get("none").is_some() {
        Vec::new()
    } else {
        return Err("select needs at, rect, polygon, refs or none".into());
    };
    scene.select(&refs);
    Ok(())
}

/// What every canvas edit answers.
fn edit_reply(scene: &Scene, dirty: Option<BBox>) -> Value {
    json!({"dirty": bbox_json(dirty), "selection": refs_json(&scene.selection),
           "selectionBounds": bbox_json(scene.bounds_of(&scene.selection)),
           "canUndo": scene.can_undo(), "canRedo": scene.can_redo()})
}

impl App {
    /// `nativeCanvasForDraw(id, x0, y0, x1, y1)` (F21-FR1): the canvas's strokes and items
    /// meeting the world rectangle, in paint order, arrow ends resolved, with the history
    /// state, the selection and whether a cleared canvas can be restored (OQ34).
    pub fn canvas_for_draw_json(&self, id: &str, rect: [f32; 4]) -> String {
        let r = rect_of(rect);
        let out = self.with_ink(id, Layer::Canvas, |o| {
            let mut v = crate::canvas::draw::for_draw(&o.scene, r);
            v["canRestore"] = json!(notes::cleared_canvas(&o.dir, id) == Some(1));
            v
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeCanvasEdit(id, opJson)` (F21): one user gesture on the canvas, one undo step.
    /// `{dirty: [x0, y0, x1, y1]|null, selection: [ref], canUndo, canRedo}` or `{error}`.
    pub fn canvas_edit_json(&self, id: &str, op: &str) -> String {
        let Ok(op) = serde_json::from_str::<Value>(op) else {
            return error_json("an edit must be JSON");
        };
        let name = op
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let out = self.with_ink(id, Layer::Canvas, |o| -> Result<Value, String> {
            if name == "select" {
                select(&mut o.scene, &op)?;
                return Ok(edit_reply(&o.scene, None));
            }
            if !o.scene.items_ok() {
                return Err("The canvas's shapes can't be read, so they can't be changed".into());
            }
            let refs = refs_of(&op, &o.scene);
            let dirty = match name.as_str() {
                // F21-FR4: the selection (or the refs given) moved, deleted or duplicated.
                "move" => {
                    let (dx, dy) = (num(&op, "dx")?, num(&op, "dy")?);
                    let all = o.scene.with_groups(&refs);
                    o.scene.move_refs(&all, dx, dy)
                }
                "delete" => {
                    let all = o.scene.with_groups(&refs);
                    o.scene.delete(&all).1
                }
                "duplicate" => o.scene.duplicate(&refs, &now_local()),
                // F21-FR5: one item's handles; arranging and grouping the selection.
                "resize" => {
                    let corner = op
                        .get("corner")
                        .and_then(Value::as_str)
                        .and_then(Corner::parse)
                        .ok_or("corner must be tl, tr, bl or br")?;
                    let m = Measured {
                        fit: op.get("fit").and_then(Value::as_f64),
                        cell_heights: floats(&op, "cellHeights")
                            .map(|v| v.into_iter().map(f64::from).collect()),
                    };
                    o.scene.resize(&one(&op)?, corner, at(&op)?, &m)
                }
                "rotate" => o.scene.rotate(&one(&op)?, at(&op)?),
                "endpoint" => {
                    let start = op.get("which").and_then(Value::as_str) == Some("start");
                    o.scene.move_end(&one(&op)?, start, at(&op)?)
                }
                "group" => o.scene.group(&refs).map(|(_, a)| a),
                "ungroup" => o.scene.ungroup(&refs),
                "front" => o.scene.arrange(&refs, true),
                "back" => o.scene.arrange(&refs, false),
                // F21-FR3: the backup goes first; without it, nothing is cleared (OQ34).
                "clear" => {
                    if o.scene.is_empty() {
                        None
                    } else {
                        notes::save_cleared(&o.dir, id, 1, &o.scene)
                            .map_err(|e| format!("The canvas can't be cleared ({:?})", e.kind()))?;
                        o.scene.clear()
                    }
                }
                "restoreCleared" => {
                    let b = notes::load_cleared(&o.dir, id, 1)
                        .ok_or("There is no cleared canvas to restore")?;
                    o.scene.absorb(&b)
                }
                other => return Err(format!("unknown edit {other:?}")),
            };
            o.dirty |= dirty.is_some();
            Ok(edit_reply(&o.scene, dirty))
        });
        match out {
            Ok(Ok(v)) => v.to_string(),
            Ok(Err(e)) | Err(e) => error_json(&e),
        }
    }
}
