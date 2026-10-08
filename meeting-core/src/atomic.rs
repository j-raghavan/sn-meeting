//! ADR D5: every file the core owns is written tmp + fsync + rename (F3-FR2, F10-FR7).
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<path>.tmp`, next to the target so the rename stays on one filesystem.
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Write `bytes` to `path` atomically. A crash leaves either the old file or the new one; a
/// stray `.tmp` is ignored by readers and overwritten by the next write (F3-AC3).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    // Persist the rename itself; best effort (some filesystems refuse to fsync a directory).
    if let Some(dir) = path.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Read at most `cap` bytes; a bigger file is not one of ours.
pub fn read_capped(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    if std::fs::metadata(path)?.len() > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
        ));
    }
    std::fs::read(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_and_leaves_no_tmp() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert!(!tmp_path(&p).exists());
        assert_eq!(tmp_path(&p), d.path().join("a.json.tmp"));
    }

    #[test]
    fn a_stray_tmp_does_not_affect_the_file() {
        // F3-AC3: a write interrupted after the tmp was written leaves the old file intact.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        write_atomic(&p, b"old").unwrap();
        std::fs::write(tmp_path(&p), b"half-writ").unwrap();
        assert_eq!(read_capped(&p, 100).unwrap(), b"old");
        write_atomic(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn caps_and_errors() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big");
        std::fs::write(&p, [0u8; 11]).unwrap();
        assert_eq!(
            read_capped(&p, 10).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(read_capped(&p, 11).unwrap().len(), 11);
        let missing = d.path().join("nope");
        assert_eq!(
            read_capped(&missing, 10).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        let no_dir = d.path().join("no/such/dir/x");
        assert!(write_atomic(&no_dir, b"x").is_err());
    }
}
