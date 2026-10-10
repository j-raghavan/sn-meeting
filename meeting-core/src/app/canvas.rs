//! F21 behind JNI: the canvas's scene (strokes and items, one history) as the shell draws and
//! edits it. The scene is the open canvas layer of `super::notes`; every edit is one step.
use super::App;
use super::notes::rect_of;
use crate::api::error_json;
use crate::canvas::Scene;
use crate::canvas::draw::{bbox_json, refs_json};
use crate::ink::BBox;
use crate::notes::{self, Layer};
use serde_json::{Value, json};

/// What every canvas edit answers.
fn edit_reply(scene: &Scene, dirty: Option<BBox>) -> Value {
    json!({"dirty": bbox_json(dirty), "selection": refs_json(&scene.selection),
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
        let out = self.with_ink(id, Layer::Canvas, |o| -> Result<Value, String> {
            if !o.scene.items_ok() {
                return Err("The canvas's shapes can't be read, so they can't be changed".into());
            }
            let dirty = match op.get("op").and_then(Value::as_str).unwrap_or("") {
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
