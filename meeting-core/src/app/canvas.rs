//! F21 behind JNI: the canvas's scene (strokes and items, one history) as the shell draws and
//! edits it. The scene is the open canvas layer of `super::notes`; every edit is one step.
use super::App;
use super::notes::rect_of;
use crate::api::error_json;
use crate::canvas::draw::{bbox_json, refs_json};
use crate::canvas::edits::{Corner, Made, Measured};
use crate::canvas::geom::Pt;
use crate::canvas::table::TableEdit;
use crate::canvas::{Ref, Scene, Style};
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

/// A style pick: `{property, value}` (a number value is taken as its text).
fn pick(op: &Value) -> Result<(String, String), String> {
    let p = op
        .get("property")
        .and_then(Value::as_str)
        .ok_or("property is needed")?;
    let v = match op.get("value") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err("value is needed".into()),
    };
    Ok((p.to_owned(), v))
}

/// A world point given as `[x, y]`.
fn pair(op: &Value, k: &str) -> Result<Pt, String> {
    let v = floats(op, k)
        .filter(|v| v.len() == 2)
        .ok_or_else(|| format!("{k} must be [x, y]"))?;
    Ok(Pt::new(f64::from(v[0]), f64::from(v[1])))
}

/// A row or column index of an op.
fn index(op: &Value, k: &str) -> Result<usize, String> {
    op.get(k)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("{k} must be a whole number"))
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
    /// The meeting's folder (where its notes folder is).
    fn meeting_dir(&self, id: &str) -> Result<std::path::PathBuf, String> {
        if !crate::sidecar::is_valid_id(id) {
            return Err("invalid meeting id".into());
        }
        self.library()
            .find(id)
            .map(|m| m.dir)
            .map_err(|e| e.to_string())
    }

    /// `nativeCanvasList(id)` (F21-FR12): `{current, canvases: [{n, createdAt, strokes,
    /// items}]}`, for ⋮ → Canvases….
    pub fn canvas_list_json(&self, id: &str) -> String {
        let dir = match self.meeting_dir(id) {
            Ok(d) => d,
            Err(e) => return error_json(&e),
        };
        let list = notes::read_canvases(&dir, id);
        let open = self.ink.lock();
        let rows: Vec<Value> = list
            .list
            .iter()
            .map(|(n, at)| {
                let counts = match open
                    .layers
                    .get(&(id.to_owned(), super::notes::Slot::Canvas(*n)))
                {
                    Some(o) => Some((o.scene.ink().strokes().len(), o.scene.doc().items.len())),
                    None => notes::load_scene(&dir, id, *n)
                        .ok()
                        .map(|(s, _)| (s.ink().strokes().len(), s.doc().items.len())),
                };
                let (strokes, items) =
                    counts.map_or((Value::Null, Value::Null), |(a, b)| (json!(a), json!(b)));
                json!({"n": n, "createdAt": at, "strokes": strokes, "items": items})
            })
            .collect();
        json!({"current": list.current, "canvases": rows}).to_string()
    }

    /// `nativeCanvasNew(id)` (F21-FR12): saves the canvas shown, then a new empty canvas
    /// (`canvas-<n>/`) is shown; not an undo step. `{n}`.
    pub fn canvas_new_json(&self, id: &str) -> String {
        let dir = match self.meeting_dir(id) {
            Ok(d) => d,
            Err(e) => return error_json(&e),
        };
        if let (_, Some(e)) = self.flush_ink(Some(id)) {
            return error_json(&e);
        }
        let mut list = notes::read_canvases(&dir, id);
        let n = list.list.iter().map(|c| c.0).max().unwrap_or(1) + 1;
        if n > notes::MAX_CANVASES {
            return error_json("This meeting has as many canvases as it can hold");
        }
        list.list.push((n, now_local()));
        list.current = n;
        let made = std::fs::create_dir_all(notes::canvas_dir(&dir, id, n))
            .and_then(|()| notes::write_canvases(&dir, id, &list));
        if let Err(e) = made {
            return error_json(&format!("A new canvas can't be made ({:?})", e.kind()));
        }
        self.ink.lock().current.insert(id.to_owned(), n);
        json!({"n": n}).to_string()
    }

    /// `nativeCanvasShow(id, n)` (F21-FR12): save the canvas shown and show canvas `n`; later
    /// ink and edit calls act on it, and the meeting reopens on it. Replies as
    /// `nativeNotesOpen` does, with `n`.
    pub fn canvas_show_json(&self, id: &str, n: i32) -> String {
        let dir = match self.meeting_dir(id) {
            Ok(d) => d,
            Err(e) => return error_json(&e),
        };
        let mut list = notes::read_canvases(&dir, id);
        let Some(n) = u32::try_from(n)
            .ok()
            .filter(|n| list.list.iter().any(|c| c.0 == *n))
        else {
            return error_json("There is no such canvas");
        };
        if let (_, Some(e)) = self.flush_ink(Some(id)) {
            return error_json(&e);
        }
        if list.current != n {
            list.current = n;
            if let Err(e) = notes::write_canvases(&dir, id, &list) {
                return error_json(&format!("The canvas can't be shown ({:?})", e.kind()));
            }
        }
        self.ink.lock().current.insert(id.to_owned(), n);
        let mut v: Value = serde_json::from_str(&self.notes_open_json(id)).unwrap_or(Value::Null);
        if v.get("error").is_none() {
            v["n"] = json!(n);
            v["defaultStyle"] = self.default_style().to_value();
        }
        v.to_string()
    }

    /// F21-FR12: delete canvas `n` from Canvases… while it is still empty (never canvas 1,
    /// nor the one shown).
    fn delete_canvas(&self, id: &str, n: u32) -> Result<(), String> {
        let dir = self.meeting_dir(id)?;
        let mut list = notes::read_canvases(&dir, id);
        if n == 1 || n == list.current || !list.list.iter().any(|c| c.0 == n) {
            return Err("That canvas can't be deleted".into());
        }
        let (scene, _) = notes::load_scene(&dir, id, n)?;
        if !scene.is_empty() {
            return Err("Only an empty canvas can be deleted".into());
        }
        list.list.retain(|c| c.0 != n);
        notes::write_canvases(&dir, id, &list).map_err(|e| format!("{:?}", e.kind()))?;
        self.ink
            .lock()
            .layers
            .remove(&(id.to_owned(), super::notes::Slot::Canvas(n)));
        let _ = std::fs::remove_dir_all(notes::canvas_dir(&dir, id, n));
        Ok(())
    }
    /// `nativeCanvasForDraw(id, x0, y0, x1, y1)` (F21-FR1): the canvas's strokes and items
    /// meeting the world rectangle, in paint order, arrow ends resolved, with the history
    /// state, the selection and whether a cleared canvas can be restored (OQ34).
    pub fn canvas_for_draw_json(&self, id: &str, rect: [f32; 4]) -> String {
        let r = rect_of(rect);
        let out = self.with_ink(id, Layer::Canvas, |o| {
            let shown = notes::read_canvases(&o.dir, id).current;
            let mut v = crate::canvas::draw::for_draw(&o.scene, r);
            v["canRestore"] = json!(notes::cleared_canvas(&o.dir, id) == Some(shown));
            v
        });
        let out = out.map(|mut v| {
            v["defaultStyle"] = self.default_style().to_value();
            v
        });
        out.map_or_else(|e| error_json(&e), |v| v.to_string())
    }

    /// `nativeCanvasHit(id, x, y, r)` (F21-FR10, FR15): the topmost stroke or item at a world
    /// point: `{ref, cell: {row, col}|null, atMs|null}`; a timed one also gives where Listen
    /// starts (`seekMs`, `segIndex`, `segOffsetMs`, `lineN`), as `nativeInkHit` does.
    pub fn canvas_hit_json(&self, id: &str, x: f32, y: f32, r: f32) -> String {
        let p = Pt::new(f64::from(x), f64::from(y));
        let hit = self.with_ink(id, Layer::Canvas, |o| {
            let s = &o.scene;
            let rf = s.top_at(p, f64::from(r))?;
            let (cell, at) = match &rf {
                Ref::Stroke(n) => (
                    None,
                    s.ink()
                        .get(crate::ink::StrokeId(*n))
                        .and_then(|st| st.rec_ms),
                ),
                Ref::Item(i) => {
                    let it = s.item(i)?;
                    let cell = crate::canvas::table::cell_at(it, p);
                    let cell_at = cell.and_then(|(row, col)| {
                        let t = it.table.as_ref()?;
                        t.cell_rec_ms[t.index(row, col)?]
                    });
                    (cell, cell_at.or(it.rec_ms))
                }
            };
            Some((rf, cell, at))
        });
        let hit = match hit {
            Ok(h) => h,
            Err(e) => return error_json(&e),
        };
        let Some((rf, cell, at)) = hit else {
            return json!({"ref": null, "cell": null, "atMs": null}).to_string();
        };
        let mut v = json!({"ref": rf.to_string(), "atMs": at,
            "cell": cell.map(|(row, col)| json!({"row": row, "col": col}))});
        if let Some(t) = at {
            match self.library().get(id) {
                Ok(m) => {
                    let sc = m.meeting;
                    let s = notes::seek(&sc, t);
                    v["seekMs"] = json!(s.seek_ms);
                    v["segIndex"] = json!(s.seg_index);
                    v["segOffsetMs"] = json!(s.seg_offset_ms);
                    v["lineN"] = json!(super::live::line_at(&super::live::transcript_rows(&sc), t));
                }
                Err(e) => return error_json(&e.to_string()),
            }
        }
        v.to_string()
    }

    /// `nativeCanvasImportImage(id, srcPath, pxW, pxH, viewJson)` (F21-FR11): the core checks
    /// the picked file (Security), copies it to `images/<itemId>.<ext>` and places the item in
    /// the view (`viewJson`: `{rect: [x0, y0, x1, y1], recMs}`), one step. `{itemId}` (with the
    /// edit reply) or `{error}`; nothing is written when it is refused.
    pub fn canvas_import_image_json(
        &self,
        id: &str,
        src: &str,
        w: i32,
        h: i32,
        view: &str,
    ) -> String {
        let Ok(view) = serde_json::from_str::<Value>(view) else {
            return error_json("the view must be JSON");
        };
        let (Ok(w), Ok(h)) = (u32::try_from(w), u32::try_from(h)) else {
            return error_json("That image can't be read");
        };
        let rect = match floats(&view, "rect").filter(|r| r.len() == 4) {
            Some(r) => BBox {
                x0: r[0],
                y0: r[1],
                x1: r[2],
                y1: r[3],
            },
            None => return error_json("the view needs rect: [x0, y0, x1, y1]"),
        };
        let (bytes, ext) = match crate::canvas::image::read_checked(std::path::Path::new(src), w, h)
        {
            Ok(v) => v,
            Err(e) => return error_json(&e),
        };
        let made_style = self.default_style();
        let created = now_local();
        let made = Made {
            style: made_style,
            rec_ms: view.get("recMs").and_then(Value::as_u64),
            created_at: &created,
        };
        let out = self.with_ink(id, Layer::Canvas, |o| -> Result<Value, String> {
            if !o.scene.items_ok() {
                return Err("The canvas's shapes can't be read, so they can't be changed".into());
            }
            let item_id = o.scene.fresh_id();
            let file = format!("{item_id}.{ext}");
            let dir = notes::folder(&o.dir, id).join(notes::IMAGES);
            std::fs::create_dir_all(&dir)
                .and_then(|()| crate::atomic::write_atomic(&dir.join(&file), &bytes))
                .map_err(|e| format!("The image can't be copied ({:?})", e.kind()))?;
            let r = o
                .scene
                .create_image(&item_id, &file, w, h, rect, &made)
                .ok_or("The image can't be placed")?;
            o.dirty = true;
            let mut v = edit_reply(&o.scene, o.scene.bounds_of_ref(&r));
            v["itemId"] = json!(item_id);
            Ok(v)
        });
        match out {
            Ok(Ok(v)) => v.to_string(),
            Ok(Err(e)) | Err(e) => error_json(&e),
        }
    }

    /// F21-FR11: when the page closes (and its undo history goes), image copies no canvas
    /// refers to are deleted. Nothing is deleted while any `items.json` can't be read.
    pub(crate) fn clean_images(
        &self,
        id: &str,
        dir: &std::path::Path,
        open: &super::notes::Opened,
    ) {
        let mut keep = std::collections::HashSet::new();
        for (n, _) in notes::read_canvases(dir, id).list {
            let scene = match open
                .layers
                .get(&(id.to_owned(), super::notes::Slot::Canvas(n)))
            {
                Some(o) => o.scene.clone(),
                None => match notes::load_scene(dir, id, n) {
                    Ok((s, _)) => s,
                    Err(_) => return,
                },
            };
            if !scene.items_ok() {
                return;
            }
            keep.extend(scene.image_files());
        }
        let images = notes::folder(dir, id).join(notes::IMAGES);
        let Ok(list) = std::fs::read_dir(&images) else {
            return;
        };
        for e in list.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if crate::canvas::item::is_image_name(&name) && !keep.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// `nativeCanvasLinkTarget(id, ref)` (F21-FR13): where a badge tap goes: `{meetingId,
    /// exists, missing}` (a deleted meeting is `missing`); a link of a kind this build doesn't
    /// follow is `{kind, followed: false}`; no link `{meetingId: null}`.
    pub fn canvas_link_target_json(&self, id: &str, r: &str) -> String {
        let Some(rf) = Ref::parse(r) else {
            return error_json("ref must be s:<id> or i:<id>");
        };
        let link = match self.with_ink(id, Layer::Canvas, |o| o.scene.link_of(&rf).cloned()) {
            Ok(l) => l,
            Err(e) => return error_json(&e),
        };
        match link {
            None => json!({"meetingId": null}).to_string(),
            Some(l) if !l.followed() => json!({"kind": l.kind, "followed": false}).to_string(),
            Some(l) => {
                let exists = self.library().find(&l.target).is_ok();
                json!({"meetingId": l.target, "exists": exists, "missing": !exists}).to_string()
            }
        }
    }

    /// `nativeCanvasPdfPlan(id)` (F21-FR14): where the notes PDF goes (`meeting-….notes.pdf`
    /// beside the sidecar, via a tmp file in the same folder) and one page per canvas, in
    /// canvas order, each fitted to its content with its paint-ordered content to draw.
    pub fn canvas_pdf_plan_json(&self, id: &str) -> String {
        let dir = match self.meeting_dir(id) {
            Ok(d) => d,
            Err(e) => return error_json(&e),
        };
        let list = notes::read_canvases(&dir, id);
        let open = self.ink.lock();
        let mut pages = Vec::new();
        for (n, _) in &list.list {
            let scene = match open
                .layers
                .get(&(id.to_owned(), super::notes::Slot::Canvas(*n)))
            {
                Some(o) => o.scene.clone(),
                None => match notes::load_scene(&dir, id, *n) {
                    Ok((s, _)) => s,
                    Err(e) => return error_json(&e),
                },
            };
            let p = crate::canvas::pdf::fitted(scene.bounds());
            let all = BBox {
                x0: f32::MIN,
                y0: f32::MIN,
                x1: f32::MAX,
                y1: f32::MAX,
            };
            pages.push(
                json!({"n": n, "widthPt": p.width_pt, "heightPt": p.height_pt,
                "viewport": [p.x, p.y, p.scale],
                "content": crate::canvas::draw::for_draw(&scene, all)["entries"].clone()}),
            );
        }
        drop(open);
        let path = dir.join(format!("{id}.notes.pdf"));
        let tmp = crate::atomic::tmp_path(&path);
        let images = notes::folder(&dir, id).join(notes::IMAGES);
        json!({"path": path.to_string_lossy(), "tmpPath": tmp.to_string_lossy(),
               "imagesDir": images.to_string_lossy(), "pages": pages})
        .to_string()
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
        if name == "deleteCanvas" {
            let n = op
                .get("n")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            return match n.map(|n| self.delete_canvas(id, n)) {
                Some(Ok(())) => json!({"deleted": n}).to_string(),
                Some(Err(e)) => error_json(&e),
                None => error_json("n must be a canvas number"),
            };
        }
        let default_style = self.default_style();
        let created = now_local();
        let out = self.with_ink(id, Layer::Canvas, |o| -> Result<Value, String> {
            if name == "select" {
                select(&mut o.scene, &op)?;
                return Ok(edit_reply(&o.scene, None));
            }
            if !o.scene.items_ok() {
                return Err("The canvas's shapes can't be read, so they can't be changed".into());
            }
            let refs = refs_of(&op, &o.scene);
            let made_style = default_style.clone();
            let made = Made {
                style: made_style,
                rec_ms: op.get("recMs").and_then(Value::as_u64),
                created_at: &created,
            };
            let mut created_ref = None;
            let dirty = match name.as_str() {
                // F21-FR8: a shape or connector from a pen drag, in the default style.
                "create" => {
                    let kind = op.get("type").and_then(Value::as_str).unwrap_or("");
                    created_ref = match kind {
                        // F21-FR9: a text box or note with its first text.
                        "text" | "note" => {
                            let text = op.get("text").and_then(Value::as_str).unwrap_or("");
                            let h = op.get("height").and_then(Value::as_f64);
                            o.scene.create_text(kind, pair(&op, "at")?, text, h, &made)
                        }
                        // F21-FR10: a table from a drag.
                        "table" => {
                            o.scene
                                .create_table(pair(&op, "from")?, pair(&op, "to")?, &made)
                        }
                        _ => {
                            o.scene
                                .create_shape(kind, pair(&op, "from")?, pair(&op, "to")?, &made)
                        }
                    };
                    if created_ref.is_none() {
                        return Err(format!("can't make a {kind:?} there"));
                    }
                    created_ref.as_ref().and_then(|r| o.scene.bounds_of_ref(r))
                }
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
                "table.setCell" => {
                    let text = op
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or("text is needed")?;
                    let h = op.get("height").and_then(Value::as_f64);
                    o.scene.set_cell(
                        &one(&op)?,
                        index(&op, "row")?,
                        index(&op, "col")?,
                        text,
                        h,
                        made.rec_ms,
                    )
                }
                "table.addRow" => o
                    .scene
                    .edit_table(&one(&op)?, TableEdit::AddRow(index(&op, "row")?)),
                "table.addColumn" => o
                    .scene
                    .edit_table(&one(&op)?, TableEdit::AddColumn(index(&op, "col")?)),
                "table.removeRow" => o
                    .scene
                    .edit_table(&one(&op)?, TableEdit::RemoveRow(index(&op, "row")?)),
                "table.removeColumn" => o
                    .scene
                    .edit_table(&one(&op)?, TableEdit::RemoveColumn(index(&op, "col")?)),
                "table.resizeRow" => {
                    let e = TableEdit::ResizeRow(index(&op, "row")?, num(&op, "height")?);
                    o.scene.edit_table(&one(&op)?, e)
                }
                "setText" => {
                    let text = op
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or("text is needed")?;
                    let h = op.get("height").and_then(Value::as_f64);
                    o.scene.set_text(&one(&op)?, text, h, made.rec_ms)
                }
                "endpoint" => {
                    let start = op.get("which").and_then(Value::as_str) == Some("start");
                    o.scene.move_end(&one(&op)?, start, at(&op)?)
                }
                "group" => o.scene.group(&refs).map(|(_, a)| a),
                "ungroup" => o.scene.ungroup(&refs),
                // F21-FR6: a style pick restyles the selection and becomes the default.
                "restyle" | "setDefaultStyle" => {
                    let (p, v) = pick(&op)?;
                    if name == "restyle" {
                        let all = o.scene.with_groups(&refs);
                        o.scene.restyle(&all, &p, &v)
                    } else {
                        None
                    }
                }
                // F21-FR13: link the selection to another meeting, or remove its link.
                "link" => {
                    let t = op.get("target").and_then(Value::as_str).unwrap_or("");
                    let l = crate::canvas::item::Link::meeting(t).ok_or("That isn't a meeting")?;
                    o.scene.relink(&refs, Some(&l))
                }
                "unlink" => o.scene.relink(&refs, None),
                "front" => o.scene.arrange(&refs, true),
                "back" => o.scene.arrange(&refs, false),
                // F21-FR3: the backup goes first; without it, nothing is cleared (OQ34).
                "clear" => {
                    if o.scene.is_empty() {
                        None
                    } else {
                        let shown = notes::read_canvases(&o.dir, id).current;
                        notes::save_cleared(&o.dir, id, shown, &o.scene)
                            .map_err(|e| format!("The canvas can't be cleared ({:?})", e.kind()))?;
                        o.scene.clear()
                    }
                }
                "restoreCleared" => {
                    let shown = notes::read_canvases(&o.dir, id).current;
                    let b = notes::load_cleared(&o.dir, id, shown)
                        .ok_or("There is no cleared canvas to restore")?;
                    o.scene.absorb(&b)
                }
                other => return Err(format!("unknown edit {other:?}")),
            };
            o.dirty |= dirty.is_some();
            let mut reply = edit_reply(&o.scene, dirty);
            if let Some(r) = created_ref {
                reply["created"] = json!(r.to_string());
            }
            Ok(reply)
        });
        let reply = match out {
            Ok(Ok(v)) => v,
            Ok(Err(e)) | Err(e) => return error_json(&e),
        };
        if (name == "restyle" || name == "setDefaultStyle")
            && let Ok((p, v)) = pick(&op)
            && let Err(e) = self.set_default_style(&p, &v)
        {
            return e;
        }
        reply.to_string()
    }

    /// F21-FR6: the style new items and strokes take (`settings.json` → `notes.defaultStyle`).
    pub(crate) fn default_style(&self) -> Style {
        self.settings()
            .0
            .notes
            .extra
            .get("defaultStyle")
            .map(Style::from_value)
            .unwrap_or_default()
    }

    /// F21-FR6: a pick becomes the default, across meetings and restarts.
    fn set_default_style(&self, property: &str, value: &str) -> Result<(), String> {
        let next = self.default_style().with(property, value);
        if next == self.default_style() {
            return Ok(());
        }
        self.commit_settings(|cur| {
            let mut n = cur.clone();
            n.notes.extra.insert("defaultStyle".into(), next.to_value());
            Ok(n)
        })
    }
}
