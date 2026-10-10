//! F17-FR5: a meeting's notes beside it in the save folder, `meeting-….notes/`:
//! `canvas.inkbin` (the Notes tab's infinite canvas, one world-space layer), `minutes.inkbin`
//! (the Minutes tab's ink area, F17-FR10) and `view.json` (the canvas's last viewport). The
//! ink files change only from user input (INV12); every write is atomic (tmp, fsync, rename).
//!
//! F21-FR1 (D21): the canvas's items, per-stroke extras and paint order go in a sibling
//! `items.json`, so `canvas.inkbin` stays v2 and a build without F21 keeps reading it. Canvas
//! n ≥ 2 of a meeting lives in `canvas-<n>/` with the same files (F21-FR12, OQ32).
pub mod png;
pub mod regions;

use crate::ink::{self, InkLayer};
use crate::sidecar::Sidecar;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The canvas's file in the notes folder.
pub const CANVAS: &str = "canvas.inkbin";
/// The Minutes tab's ink area (F17-FR10).
pub const MINUTES: &str = "minutes.inkbin";
/// The canvas's last viewport.
pub const VIEW: &str = "view.json";
/// F21-FR1: the canvas's items beside its strokes.
pub const ITEMS: &str = "items.json";
/// F21-FR11: copies of inserted images, shared by every canvas of the meeting.
pub const IMAGES: &str = "images";
/// The sidecar's `notes.format` (OQ14/OQ28 proposed: `.inkbin` v2).
pub const FORMAT: &str = "inkbin-2";
/// A notes file bigger than this is not ours (a dense 3-hour canvas is a few MB).
pub const MAX_INK_BYTES: u64 = 64 * 1024 * 1024;
/// The zoom range of the canvas (F13-FR1: 5 %–2000 %, sn-canvas `CanvasCore.kt:17-20`).
pub const MIN_ZOOM: f64 = 0.05;
pub const MAX_ZOOM: f64 = 20.0;

/// Which ink layer of a meeting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The Notes tab's canvas.
    Canvas,
    /// The Minutes tab's "Your notes on the minutes" area.
    Minutes,
}

impl Layer {
    /// `canvas` or `minutes` (the JNI name).
    pub fn from_id(s: &str) -> Option<Layer> {
        match s {
            "canvas" => Some(Layer::Canvas),
            "minutes" => Some(Layer::Minutes),
            _ => None,
        }
    }

    fn file(self) -> &'static str {
        match self {
            Layer::Canvas => CANVAS,
            Layer::Minutes => MINUTES,
        }
    }
}

/// The notes folder of `id` in the meeting's folder `dir`.
pub fn folder(dir: &Path, id: &str) -> PathBuf {
    dir.join(crate::notes_text::notes_dir_name(id))
}

/// Where `layer` of `id` lives.
pub fn path(dir: &Path, id: &str, layer: Layer) -> PathBuf {
    folder(dir, id).join(layer.file())
}

/// Read `layer`. A missing file is an empty layer; one that can't be read or decoded is an
/// error, and is then never overwritten by a save of an empty layer (the caller refuses ink).
pub fn load(dir: &Path, id: &str, layer: Layer) -> Result<InkLayer, String> {
    match crate::atomic::read_capped(&path(dir, id, layer), MAX_INK_BYTES) {
        Ok(b) => ink::decode_layer(&b).map_err(|e| format!("The notes can't be read: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(InkLayer::new()),
        Err(e) => Err(format!("The notes can't be read ({:?})", e.kind())),
    }
}

/// F21-FR12: the folder canvas `n` of `id` lives in: the notes folder for canvas 1 (today's
/// paths), `canvas-<n>/` inside it for the others.
pub fn canvas_dir(dir: &Path, id: &str, n: u32) -> PathBuf {
    let f = folder(dir, id);
    if n <= 1 {
        f
    } else {
        f.join(format!("canvas-{n}"))
    }
}

/// F21-FR1: canvas `n` as a scene: its strokes (an unreadable `.inkbin` is an error, as
/// [`load`]) and its `items.json` (missing: no items; unreadable: the scene refuses item
/// edits and never writes it).
pub fn load_scene(
    dir: &Path,
    id: &str,
    n: u32,
) -> Result<(crate::canvas::Scene, crate::canvas::LoadReport), String> {
    let at = canvas_dir(dir, id, n);
    let ink = match crate::atomic::read_capped(&at.join(CANVAS), MAX_INK_BYTES) {
        Ok(b) => ink::decode_layer(&b).map_err(|e| format!("The notes can't be read: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => InkLayer::new(),
        Err(e) => return Err(format!("The notes can't be read ({:?})", e.kind())),
    };
    let items =
        match crate::atomic::read_capped(&at.join(ITEMS), crate::canvas::scene::MAX_ITEMS_BYTES) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{:?}", e.kind())),
        };
    Ok(crate::canvas::Scene::load(ink, items))
}

/// F21-FR1: write what changed of canvas `n`: `canvas.inkbin` only when strokes changed,
/// `items.json` when it is needed and differs. Whether anything was written.
pub fn save_scene(
    dir: &Path,
    id: &str,
    n: u32,
    scene: &mut crate::canvas::Scene,
) -> std::io::Result<bool> {
    if !crate::sidecar::is_valid_id(id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid meeting id",
        ));
    }
    let at = canvas_dir(dir, id, n);
    let items = scene.items_to_save();
    if !scene.strokes_dirty() && items.is_none() {
        return Ok(false);
    }
    std::fs::create_dir_all(&at)?;
    if scene.strokes_dirty() {
        crate::atomic::write_atomic(&at.join(CANVAS), &ink::encode_layer(scene.ink()))?;
        scene.strokes_saved();
    }
    if let Some(b) = items {
        crate::atomic::write_atomic(&at.join(ITEMS), &b)?;
        scene.items_saved(b);
    }
    Ok(true)
}

/// OQ34: the hidden backup Clear canvas keeps until the next Clear, so it can be undone after
/// a restart: `cleared/` with the canvas's `canvas.inkbin`, `items.json`, the images its
/// items show and `cleared.json` (which canvas, when).
pub const CLEARED: &str = "cleared";
const CLEARED_META: &str = "cleared.json";

/// Keep canvas `n`'s content as the one backup, replacing any earlier one.
pub fn save_cleared(
    dir: &Path,
    id: &str,
    n: u32,
    scene: &crate::canvas::Scene,
) -> std::io::Result<()> {
    let notes = folder(dir, id);
    let to = notes.join(CLEARED);
    match std::fs::remove_dir_all(&to) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::fs::create_dir_all(to.join(IMAGES))?;
    crate::atomic::write_atomic(&to.join(CANVAS), &ink::encode_layer(scene.ink()))?;
    crate::atomic::write_atomic(&to.join(ITEMS), &scene.items_json())?;
    for it in &scene.doc().items {
        if let Some(img) = &it.image {
            let from = notes.join(IMAGES).join(&img.file);
            if from.is_file() {
                std::fs::copy(&from, to.join(IMAGES).join(&img.file))?;
            }
        }
    }
    let at = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let meta = json!({"version": 1, "canvas": n, "clearedAt": at});
    crate::atomic::write_atomic(&to.join(CLEARED_META), meta.to_string().as_bytes())
}

/// Which canvas the backup holds, if there is a readable one.
pub fn cleared_canvas(dir: &Path, id: &str) -> Option<u32> {
    let b =
        crate::atomic::read_capped(&folder(dir, id).join(CLEARED).join(CLEARED_META), 4096).ok()?;
    let v: Value = serde_json::from_slice(&b).ok()?;
    v.get("canvas")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// The backup of canvas `n` as a scene, its images copied back into `images/` where they
/// are missing; `None` without one.
pub fn load_cleared(dir: &Path, id: &str, n: u32) -> Option<crate::canvas::Scene> {
    if cleared_canvas(dir, id) != Some(n) {
        return None;
    }
    let notes = folder(dir, id);
    let from = notes.join(CLEARED);
    let ink =
        ink::decode_layer(&crate::atomic::read_capped(&from.join(CANVAS), MAX_INK_BYTES).ok()?)
            .ok()?;
    let items =
        crate::atomic::read_capped(&from.join(ITEMS), crate::canvas::scene::MAX_ITEMS_BYTES).ok();
    let (scene, _) = crate::canvas::Scene::load(ink, Ok(items));
    for it in &scene.doc().items {
        if let Some(img) = &it.image {
            let to = notes.join(IMAGES).join(&img.file);
            if !to.exists() && std::fs::create_dir_all(notes.join(IMAGES)).is_ok() {
                let _ = std::fs::copy(from.join(IMAGES).join(&img.file), &to);
            }
        }
    }
    Some(scene)
}

/// Write `layer` atomically, creating the notes folder if needed.
pub fn save(dir: &Path, id: &str, layer: Layer, ink: &InkLayer) -> std::io::Result<()> {
    if !crate::sidecar::is_valid_id(id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid meeting id",
        ));
    }
    std::fs::create_dir_all(folder(dir, id))?;
    crate::atomic::write_atomic(&path(dir, id, layer), &ink::encode_layer(ink))
}

/// The canvas's viewport: the world point at the view's centre, and the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
}

impl View {
    /// A viewport with finite numbers and a zoom in range, else `None`.
    pub fn checked(self) -> Option<View> {
        let ok = self.cx.is_finite() && self.cy.is_finite() && self.zoom.is_finite();
        ok.then(|| View {
            zoom: self.zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            ..self
        })
    }
}

/// The last viewport, or `None` (never saved, or unreadable: the canvas then fits its ink).
pub fn read_view(dir: &Path, id: &str) -> Option<View> {
    read_view_of(dir, id, 1)
}

/// Canvas `n`'s last viewport (F21-FR12: each canvas keeps its own).
pub fn read_view_of(dir: &Path, id: &str, n: u32) -> Option<View> {
    let b = crate::atomic::read_capped(&canvas_dir(dir, id, n).join(VIEW), 4096).ok()?;
    serde_json::from_slice::<View>(&b).ok()?.checked()
}

/// Keep the viewport for next time.
pub fn write_view(dir: &Path, id: &str, v: View) -> std::io::Result<()> {
    write_view_of(dir, id, 1, v)
}

/// Keep canvas `n`'s viewport.
pub fn write_view_of(dir: &Path, id: &str, n: u32, v: View) -> std::io::Result<()> {
    let v = v.checked().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "viewport is not finite")
    })?;
    let at = canvas_dir(dir, id, n);
    std::fs::create_dir_all(&at)?;
    let bytes = serde_json::to_vec(&v).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(&at.join(VIEW), &bytes)
}

/// F21-FR12 (OQ32): the meeting's canvases, `canvases.json`: `{version: 1, current,
/// canvases: [{n, createdAt}]}`. Missing (or unreadable) means one canvas, canvas 1.
pub const CANVASES: &str = "canvases.json";
/// At most this many canvases a meeting lists.
pub const MAX_CANVASES: u32 = 999;

/// The canvases of a meeting and the one shown last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvases {
    pub current: u32,
    /// `(n, createdAt)` in canvas order.
    pub list: Vec<(u32, String)>,
}

impl Default for Canvases {
    fn default() -> Self {
        Canvases {
            current: 1,
            list: vec![(1, String::new())],
        }
    }
}

/// Read `canvases.json` (untrusted: numbers in range, no repeats, canvas 1 always there).
pub fn read_canvases(dir: &Path, id: &str) -> Canvases {
    let Ok(b) = crate::atomic::read_capped(&folder(dir, id).join(CANVASES), 256 * 1024) else {
        return Canvases::default();
    };
    let Ok(v) = serde_json::from_slice::<Value>(&b) else {
        return Canvases::default();
    };
    let mut list: Vec<(u32, String)> = v
        .get("canvases")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let n = u32::try_from(c.get("n")?.as_u64()?)
                        .ok()
                        .filter(|n| (1..=MAX_CANVASES).contains(n))?;
                    let at = c
                        .get("createdAt")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(64)
                        .collect();
                    Some((n, at))
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|c| c.0);
    list.dedup_by_key(|c| c.0);
    if list.first().map(|c| c.0) != Some(1) {
        list.insert(0, (1, String::new()));
    }
    let current = v
        .get("current")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| list.iter().any(|c| c.0 == *n))
        .unwrap_or(1);
    Canvases { current, list }
}

/// Write `canvases.json`.
pub fn write_canvases(dir: &Path, id: &str, c: &Canvases) -> std::io::Result<()> {
    let list: Vec<Value> = c
        .list
        .iter()
        .map(|(n, at)| json!({"n": n, "createdAt": at}))
        .collect();
    let v = json!({"version": 1, "current": c.current, "canvases": list});
    std::fs::create_dir_all(folder(dir, id))?;
    crate::atomic::write_atomic(&folder(dir, id).join(CANVASES), v.to_string().as_bytes())
}

/// The sidecar's `notes` object (Data Model): the folder, the format, the coordinate space,
/// how stroke times are encoded (OQ14) and the Minutes ink file.
pub fn field(id: &str) -> Value {
    json!({"dir": crate::notes_text::notes_dir_name(id), "format": FORMAT, "space": "world",
           "clock": "recording-ms", "minutesInk": MINUTES})
}

/// F19-FR7: when the notes were last recognised (`notesRecognisedAt`), as epoch ms.
pub fn recognised_at_ms(sc: &Sidecar) -> Option<u64> {
    let t = sc.extra.get("notesRecognisedAt")?.as_str()?;
    let d = chrono::DateTime::parse_from_rfc3339(t).ok()?;
    u64::try_from(d.timestamp_millis()).ok()
}

/// F17-FR9 / F21-FR15: what the Stop call sends of a meeting's canvases: the regions of every
/// canvas's strokes as PNGs (shapes, text and images are never drawn into them), the strokes'
/// boxes, and the typed text in reading order (canvas, then top to bottom). `None` without
/// notes; a canvas that can't be read is left out.
pub fn canvas_input(dir: &Path, sc: &Sidecar) -> Option<crate::notes_text::NotesInput> {
    if !has_notes(sc) && !folder(dir, &sc.id).is_dir() {
        return None;
    }
    let at = recognised_at_ms(sc);
    let mut out = crate::notes_text::NotesInput::default();
    for (n, _) in read_canvases(dir, &sc.id).list {
        let Ok((scene, _)) = load_scene(dir, &sc.id, n) else {
            continue;
        };
        if !scene.ink().is_empty() {
            let one = regions::input_on(scene.ink(), at, n);
            out.regions.extend(one.regions);
            out.strokes.extend(one.strokes);
        }
        out.typed.extend(
            crate::canvas::text::typed(&scene, n)
                .into_iter()
                .map(|t| t.text),
        );
    }
    (!out.regions.is_empty() || !out.typed.is_empty()).then_some(out)
}

/// Whether the meeting has notes (its sidecar names the folder).
pub fn has_notes(sc: &Sidecar) -> bool {
    sc.extra.get("notes").is_some_and(Value::is_object)
}

/// Name the notes in the sidecar, once; `true` when it changed.
pub fn declare(sc: &mut Sidecar) -> bool {
    if has_notes(sc) {
        return false;
    }
    sc.extra.insert("notes".into(), field(&sc.id));
    true
}

/// F17-FR7: how far before a stroke's time Listen starts playing.
pub const LEAD_MS: u64 = 2_000;

/// Where Listen starts playback for a stroke written at recording time `at_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seek {
    /// `max(0, at − 2 s)` on the recording clock.
    pub seek_ms: u64,
    /// The segment that holds it (0-based) and how far into that segment.
    pub seg_index: usize,
    pub seg_offset_ms: u64,
}

/// F17-FR7 / F17-AC4: `max(0, at − 2 000 ms)`, placed in the last segment starting at or
/// before it (segments are gapless on the recording clock, F15-FR2, so `startOffsetMs` places
/// any moment). A meeting without segments seeks in a first one.
pub fn seek(sc: &Sidecar, at_ms: u64) -> Seek {
    let seek_ms = at_ms.saturating_sub(LEAD_MS);
    let (seg_index, start) = sc
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start_offset_ms <= seek_ms)
        .map(|(i, s)| (i, s.start_offset_ms))
        .next_back()
        .unwrap_or((0, 0));
    Seek {
        seek_ms,
        seg_index,
        seg_offset_ms: seek_ms - start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::{InkColor, InkPoint, Tool};

    const ID: &str = "meeting-20261009-100000";

    fn one_stroke() -> InkLayer {
        let mut l = InkLayer::new();
        let p = InkPoint::new(-300.0, 9_000.0, 1.0, None, None, 0).unwrap();
        l.add(Tool::Pen, InkColor::BLACK, 3.0, vec![p], 5, Some(450_000))
            .unwrap();
        l
    }

    #[test]
    fn layers_round_trip_beside_the_meeting() {
        let d = tempfile::tempdir().unwrap();
        for layer in [Layer::Canvas, Layer::Minutes] {
            assert!(
                load(d.path(), ID, layer).unwrap().is_empty(),
                "missing: empty"
            );
            save(d.path(), ID, layer, &one_stroke()).unwrap();
            let back = load(d.path(), ID, layer).unwrap();
            assert_eq!(back.strokes(), one_stroke().strokes());
        }
        assert!(
            d.path()
                .join("meeting-20261009-100000.notes/canvas.inkbin")
                .is_file()
        );
        assert!(
            d.path()
                .join("meeting-20261009-100000.notes/minutes.inkbin")
                .is_file()
        );
        assert!(save(d.path(), "../x", Layer::Canvas, &one_stroke()).is_err());
        assert_eq!(Layer::from_id("canvas"), Some(Layer::Canvas));
        assert_eq!(Layer::from_id("minutes"), Some(Layer::Minutes));
        assert_eq!(Layer::from_id("page"), None);
    }

    #[test]
    fn a_damaged_or_unreadable_layer_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(folder(d.path(), ID)).unwrap();
        std::fs::write(path(d.path(), ID, Layer::Canvas), b"INKB\x01").unwrap();
        assert!(
            load(d.path(), ID, Layer::Canvas)
                .unwrap_err()
                .contains("can't be read")
        );
        std::fs::create_dir_all(path(d.path(), ID, Layer::Minutes)).unwrap();
        assert!(
            load(d.path(), ID, Layer::Minutes).is_err(),
            "a folder in its place"
        );
    }

    /// F21-FR1 / FR12: scenes load from and save to canvas n's folder; a damaged `.inkbin`
    /// refuses the scene, and nothing is written for a bad id.
    #[test]
    fn scenes_live_in_their_canvas_folder() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(canvas_dir(d.path(), ID, 1), folder(d.path(), ID));
        assert_eq!(
            canvas_dir(d.path(), ID, 3),
            folder(d.path(), ID).join("canvas-3")
        );
        let (mut s, _) = load_scene(d.path(), ID, 2).unwrap();
        assert!(s.is_empty());
        let p = InkPoint::new(1.0, 2.0, 1.0, None, None, 0).unwrap();
        let st = s
            .new_stroke(Tool::Pen, InkColor::BLACK, 2.0, vec![p], 1, Some(9))
            .unwrap();
        s.add_stroke(st, None);
        assert!(save_scene(d.path(), "../x", 2, &mut s).is_err());
        assert!(save_scene(d.path(), ID, 2, &mut s).unwrap());
        assert!(!save_scene(d.path(), ID, 2, &mut s).unwrap());
        let (back, _) = load_scene(d.path(), ID, 2).unwrap();
        assert_eq!(back.ink().strokes()[0].rec_ms, Some(9));
        std::fs::write(canvas_dir(d.path(), ID, 2).join(CANVAS), b"INKB\x09").unwrap();
        assert!(
            load_scene(d.path(), ID, 2)
                .unwrap_err()
                .contains("can't be read")
        );
        std::fs::remove_file(canvas_dir(d.path(), ID, 2).join(CANVAS)).unwrap();
        std::fs::create_dir_all(canvas_dir(d.path(), ID, 2).join(CANVAS)).unwrap();
        assert!(
            load_scene(d.path(), ID, 2).is_err(),
            "a folder in its place"
        );
    }

    /// F21-FR12: the canvas list; anything odd reads as one canvas, canvas 1 always there.
    #[test]
    fn canvases_are_listed_and_checked() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_canvases(d.path(), ID), Canvases::default());
        let c = Canvases {
            current: 3,
            list: vec![(1, "a".into()), (3, "b".into())],
        };
        write_canvases(d.path(), ID, &c).unwrap();
        assert_eq!(read_canvases(d.path(), ID), c);
        let f = folder(d.path(), ID).join(CANVASES);
        std::fs::write(
            &f,
            br#"{"current": 9, "canvases": [{"n": 2}, {"n": 2}, {"n": 0}, {"x": 1}]}"#,
        )
        .unwrap();
        let odd = read_canvases(d.path(), ID);
        assert_eq!((odd.current, odd.list.len()), (1, 2));
        std::fs::write(&f, b"junk").unwrap();
        assert_eq!(read_canvases(d.path(), ID), Canvases::default());
        let v = View {
            cx: 1.0,
            cy: 2.0,
            zoom: 1.0,
        };
        write_view_of(d.path(), ID, 2, v).unwrap();
        assert_eq!(read_view_of(d.path(), ID, 2), Some(v));
        assert_eq!(read_view(d.path(), ID), None);
    }

    #[test]
    fn the_viewport_is_kept_and_checked() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_view(d.path(), ID), None);
        let v = View {
            cx: -1_200.5,
            cy: 30_000.0,
            zoom: 40.0,
        };
        write_view(d.path(), ID, v).unwrap();
        assert_eq!(read_view(d.path(), ID), Some(View { zoom: 20.0, ..v }));
        let nan = View { cx: f64::NAN, ..v };
        assert!(write_view(d.path(), ID, nan).is_err());
        std::fs::write(folder(d.path(), ID).join(VIEW), b"{\"cx\":1}").unwrap();
        assert_eq!(read_view(d.path(), ID), None);
    }

    #[test]
    fn the_sidecar_names_the_notes_once() {
        let mut sc: Sidecar = serde_json::from_value(json!({
            "schema": 1, "id": ID, "startedAt": "2026-10-09T10:00:00-07:00",
            "status": "recording"}))
        .unwrap();
        assert!(!has_notes(&sc));
        assert!(declare(&mut sc));
        assert!(!declare(&mut sc));
        assert_eq!(
            sc.extra["notes"],
            json!({"dir": "meeting-20261009-100000.notes", "format": "inkbin-2",
                   "space": "world", "clock": "recording-ms", "minutesInk": "minutes.inkbin"})
        );
    }

    /// F17-AC4: 0:07:30 in segment 1 seeks to 0:07:28; 0:21:10 in segment 2 (from 0:15:00)
    /// to 0:21:08, 6:08 into it; under 2 s seeks to 0.
    #[test]
    fn listen_seeks_two_seconds_back_across_segments() {
        let sc: Sidecar = serde_json::from_value(json!({
            "schema": 1, "id": ID, "startedAt": "2026-10-09T10:00:00-07:00", "status": "done",
            "segments": [{"file": "a-01.m4a", "startOffsetMs": 0, "durationMs": 900_000},
                         {"file": "a-02.m4a", "startOffsetMs": 900_000, "durationMs": 900_000}]}))
        .unwrap();
        let s = seek(&sc, 450_000);
        assert_eq!(
            (s.seek_ms, s.seg_index, s.seg_offset_ms),
            (448_000, 0, 448_000)
        );
        let s = seek(&sc, 1_270_000);
        assert_eq!(
            (s.seek_ms, s.seg_index, s.seg_offset_ms),
            (1_268_000, 1, 368_000)
        );
        let s = seek(&sc, 1_500);
        assert_eq!((s.seek_ms, s.seg_index, s.seg_offset_ms), (0, 0, 0));
        let none: Sidecar = serde_json::from_value(json!({
            "schema": 1, "id": ID, "startedAt": "2026-10-09T10:00:00-07:00", "status": "done"}))
        .unwrap();
        assert_eq!(seek(&none, 9_000).seg_index, 0);
    }
}
