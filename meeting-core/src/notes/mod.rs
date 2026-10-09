//! F17-FR5: a meeting's notes beside it in the save folder, `meeting-….notes/`:
//! `canvas.inkbin` (the Notes tab's infinite canvas, one world-space layer), `minutes.inkbin`
//! (the Minutes tab's ink area, F17-FR10) and `view.json` (the canvas's last viewport). The
//! ink files change only from user input (INV12); every write is atomic (tmp, fsync, rename).
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
    let b = crate::atomic::read_capped(&folder(dir, id).join(VIEW), 4096).ok()?;
    serde_json::from_slice::<View>(&b).ok()?.checked()
}

/// Keep the viewport for next time.
pub fn write_view(dir: &Path, id: &str, v: View) -> std::io::Result<()> {
    let v = v.checked().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "viewport is not finite")
    })?;
    std::fs::create_dir_all(folder(dir, id))?;
    let bytes = serde_json::to_vec(&v).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(&folder(dir, id).join(VIEW), &bytes)
}

/// The sidecar's `notes` object (Data Model): the folder, the format, the coordinate space,
/// how stroke times are encoded (OQ14) and the Minutes ink file.
pub fn field(id: &str) -> Value {
    json!({"dir": crate::notes_text::notes_dir_name(id), "format": FORMAT, "space": "world",
           "clock": "recording-ms", "minutesInk": MINUTES})
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
}
