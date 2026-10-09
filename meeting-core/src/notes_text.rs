//! F19-FR6: `meeting-….notes/notes-text.json`, the handwriting recognised at Stop, line by line,
//! in world coordinates. Derived data: the index rebuilds from it without the AI (INV17), and
//! it is never ink (INV12). Unknown fields survive a rewrite, as in the sidecar (F3-FR6).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The file's name inside the meeting's notes folder.
pub const FILE: &str = "notes-text.json";
/// Recognised text for a whole canvas is a few hundred KB at most; bigger is not ours.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// `[x0, y0, x1, y1]`: world coordinates in a region's `rect` and a line's `box`.
pub type Rect = [f64; 4];

/// One region: a cluster of strokes sent as one image, and the lines read from it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// The region's number in the call that recognised it (0-based).
    pub region: u32,
    /// The region's world rectangle.
    pub rect: Rect,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One recognised line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    /// The line's world box.
    #[serde(rename = "box")]
    pub bbox: Rect,
    /// The strokes the box covers (inkread `StrokeId`s).
    #[serde(default)]
    pub stroke_ids: Vec<u32>,
    /// The earliest of those strokes' recording-clock times; `null` when untimed.
    #[serde(default)]
    pub at_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The notes folder of meeting `id` (Data Model: `meeting-YYYYMMDD-HHMMSS.notes`).
pub fn notes_dir_name(id: &str) -> String {
    format!("{id}.notes")
}

/// Where `id`'s recognised text lives, in the meeting's folder `dir`.
pub fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join(notes_dir_name(id)).join(FILE)
}

/// Read a `notes-text.json`. A missing file is no regions; one that can't be read or parsed is
/// an error (the index then holds no notes for that meeting until it is fixed).
pub fn read(path: &Path) -> Result<Vec<Region>, String> {
    let bytes = match crate::atomic::read_capped(path, MAX_BYTES) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("notes text unreadable: {:?}", e.kind())),
    };
    serde_json::from_slice(&bytes).map_err(|e| format!("notes text is not valid: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_data_model_shape_and_keeps_unknown_fields() {
        let d = tempfile::tempdir().unwrap();
        let id = "meeting-20261009-100000";
        let p = path(d.path(), id);
        assert_eq!(read(&p).unwrap(), vec![]);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let doc = json!([{"region": 0, "rect": [0.0, 0.0, 100.0, 50.0], "future": 1,
            "lines": [{"text": "Q3 budget", "box": [1.0, 2.0, 3.0, 4.0], "strokeIds": [4, 5],
                       "atMs": 761000, "x": true}]}]);
        std::fs::write(&p, doc.to_string()).unwrap();
        let r = read(&p).unwrap();
        assert_eq!(r[0].lines[0].stroke_ids, vec![4, 5]);
        assert_eq!(r[0].lines[0].at_ms, Some(761_000));
        assert_eq!(serde_json::to_value(&r).unwrap(), doc);
        std::fs::write(&p, "{").unwrap();
        assert!(read(&p).unwrap_err().starts_with("notes text is not valid"));
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        assert!(read(&p).unwrap_err().starts_with("notes text unreadable"));
    }
}
