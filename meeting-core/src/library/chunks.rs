//! F15-FR5 / FR8: the near-live chunks in the library. The shell writes each chunk file in
//! `<filesDir>/chunks/<id>/c-NNNN.m4a` (app-private, never synced) and reports it; the core
//! records it in the sidecar and deletes the file once it is no longer needed: its text is
//! stored and its segment closed with a measured duration. Until then, a crash leaves the
//! finished chunks of the segment being written, from which recovery rebuilds it.
use super::{LibError, Library, invalid};
use crate::sidecar::{self, Chunk, ChunkState, Sidecar};
use crate::status::Status;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The most chunks one meeting may have (24 h at ~25 s each).
pub const MAX_CHUNKS: i32 = 4_000;
/// The longest a chunk may be: 35 s plus the 1 s it may repeat, with room for rounding.
pub const MAX_CHUNK_MS: i64 = 60_000;
/// More overlap than this is not a near-live cut (INV11: about 1 s).
const MAX_OVERLAP_MS: u64 = 2_000;

/// `c-0007.m4a`.
pub fn chunk_file_name(index: u32) -> String {
    format!("c-{index:04}.m4a")
}

/// `<root>/<id>`: one meeting's chunk folder; `root` is `<filesDir>/chunks`.
pub fn chunk_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

/// The file of chunk `index` of `id`.
pub fn chunk_path(root: &Path, id: &str, index: u32) -> PathBuf {
    chunk_dir(root, id).join(chunk_file_name(index))
}

/// `nativeChunkReady`'s arguments.
#[derive(Clone, Copy, Debug)]
pub struct ChunkIn<'a> {
    pub index: i32,
    pub path: &'a str,
    pub start_offset_ms: i64,
    pub duration_ms: i64,
    /// -1 when unknown (a chunk re-cut from its segment).
    pub speech_ms: i64,
}

/// `nativeChunkReady`'s reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkReadied {
    pub index: u32,
    pub state: ChunkState,
    /// The chunk is new (false: a known chunk's file was written again, F15-FR9's re-cut).
    pub added: bool,
}

/// The segment that holds `start_ms`: the last one starting at or before it.
fn segment_of(sc: &Sidecar, start_ms: u64) -> usize {
    sc.segments
        .iter()
        .rposition(|s| s.start_offset_ms <= start_ms)
        .unwrap_or(0)
}

/// One piece of a segment rebuilt from its chunks (F15-FR8): the chunk file, and how much of
/// its start the previous chunk already holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub path: String,
    pub skip_ms: u64,
}

/// The finalised chunk files of segment `seg`, in order, for rebuilding it after a crash.
pub(crate) fn pieces(root: &Path, sc: &Sidecar, seg: usize) -> Vec<Piece> {
    sc.chunks
        .iter()
        .filter(|c| c.seg_index == seg)
        .map(|c| (c, chunk_path(root, &sc.id, c.index)))
        .filter(|(_, p)| p.is_file())
        .map(|(c, p)| Piece {
            path: p.to_string_lossy().into_owned(),
            skip_ms: c.overlap_ms,
        })
        .collect()
}

impl Library<'_> {
    /// F15-FR5: a chunk file is finalised. While recording it is recorded `pending` (its
    /// segment and its overlap with the one before worked out here); afterwards only a known
    /// chunk may be reported, as its file re-cut from the segment (F15-FR9/FR10), which changes
    /// nothing in the sidecar. The path must be the chunk's own file under `root`.
    pub fn chunk_ready(
        &self,
        id: &str,
        root: &Path,
        c: ChunkIn<'_>,
    ) -> Result<ChunkReadied, LibError> {
        let index = u32::try_from(c.index)
            .ok()
            .filter(|_| c.index < MAX_CHUNKS)
            .ok_or_else(|| invalid("chunk index is out of range"))?;
        if !sidecar::is_valid_id(id) {
            return Err(LibError::BadId);
        }
        let expected = chunk_path(root, id, index);
        if Path::new(c.path) != expected {
            return Err(invalid(format!(
                "chunk {index} must be {}",
                expected.display()
            )));
        }
        let start =
            u64::try_from(c.start_offset_ms).map_err(|_| invalid("startOffsetMs must be >= 0"))?;
        let duration = u64::try_from(c.duration_ms)
            .ok()
            .filter(|d| *d > 0 && c.duration_ms <= MAX_CHUNK_MS)
            .ok_or_else(|| invalid("durationMs is out of range"))?;
        let (dir, mut sc) = self.load_for_write(id)?;
        if let Some(known) = sc.chunks.iter().find(|k| k.index == index) {
            return Ok(ChunkReadied {
                index,
                state: known.state,
                added: false,
            });
        }
        if sc.status != Status::Recording {
            return Err(invalid(format!(
                "chunk {index} is not part of this meeting"
            )));
        }
        if sc.chunks.last().is_some_and(|l| l.index > index) {
            return Err(invalid(format!("chunk {index} is out of order")));
        }
        let overlap = sc
            .chunks
            .last()
            .map_or(0, |l| {
                (l.start_offset_ms + l.duration_ms).saturating_sub(start)
            })
            .min(MAX_OVERLAP_MS);
        sc.chunks.push(Chunk {
            index,
            seg_index: segment_of(&sc, start),
            start_offset_ms: start,
            duration_ms: duration,
            overlap_ms: overlap,
            state: ChunkState::Pending,
            text: None,
            attempts: 0,
            last_error: None,
            speech_ms: u64::try_from(c.speech_ms).ok(),
            extra: Default::default(),
        });
        sidecar::write(&dir, &sc)?;
        Ok(ChunkReadied {
            index,
            state: ChunkState::Pending,
            added: true,
        })
    }

    /// F15-FR8: delete the chunk files no longer needed: the chunk's text is stored (or it is
    /// `missing`) and its segment is closed with a measured duration. Once a meeting past
    /// recording has none left, its folder goes too. Returns how many files were deleted.
    /// Read-only on the sidecar.
    pub fn prune_chunks(&self, id: &str, root: &Path) -> Result<usize, LibError> {
        let (_, sc) = self.load_for_write(id)?;
        let mut n = 0;
        let mut kept = false;
        for c in &sc.chunks {
            let closed = sc
                .segments
                .get(c.seg_index)
                .is_some_and(|s| s.duration_ms.is_some());
            let path = chunk_path(root, id, c.index);
            if c.state != ChunkState::Pending && closed {
                if std::fs::remove_file(&path).is_ok() {
                    n += 1;
                }
            } else if path.exists() {
                kept = true;
            }
        }
        if !kept && sc.status != Status::Recording {
            let _ = std::fs::remove_dir(chunk_dir(root, id)); // only if empty
        }
        Ok(n)
    }
}

/// F7-FR4: a deleted meeting's chunk folder goes with it.
pub(crate) fn remove_all(root: &Path, id: &str) {
    if sidecar::is_valid_id(id) {
        let _ = std::fs::remove_dir_all(chunk_dir(root, id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(chunk_file_name(7), "c-0007.m4a");
        assert_eq!(chunk_file_name(12_345), "c-12345.m4a");
        let root = Path::new("/data/files/chunks");
        assert_eq!(
            chunk_path(root, "meeting-20261009-100000", 3),
            Path::new("/data/files/chunks/meeting-20261009-100000/c-0003.m4a")
        );
    }
}
