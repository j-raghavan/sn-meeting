//! ADR D5: every file the core owns is written tmp + fsync + rename (F3-FR2, F10-FR7).
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

/// A fresh `<path>.<pid>-<n>.tmp` next to the target, so the rename stays on one filesystem
/// and two writers never share a tmp file (one would rename the other's half-written bytes).
pub fn tmp_path(path: &Path) -> PathBuf {
    let n = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{}-{n}.tmp", std::process::id()));
    PathBuf::from(s)
}

/// Whether `name` is a tmp file of `target` (`<target>.tmp` from older builds, or
/// `<target>.<pid>-<n>.tmp`).
pub fn is_tmp_of(name: &str, target: &str) -> bool {
    let Some(mid) = name
        .strip_prefix(target)
        .and_then(|r| r.strip_prefix('.'))
        .and_then(|r| r.strip_suffix("tmp"))
    else {
        return false;
    };
    mid.is_empty()
        || mid
            .strip_suffix('.')
            .and_then(|m| m.split_once('-'))
            .is_some_and(|(a, b)| {
                [a, b]
                    .iter()
                    .all(|x| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
            })
}

/// A new tmp file, never an existing one: a file or symlink already at `tmp` is refused
/// (`AlreadyExists`), so a planted link can't make the write truncate what it points to.
fn create_tmp(tmp: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)
}

/// Tmp names tried before giving up when each is already taken.
const TMP_TRIES: usize = 8;

/// Write `bytes` to `path` atomically. A crash leaves either the old file or the new one; a
/// stray `.tmp` is ignored by readers (F3-AC3). Concurrent writers each use their own tmp, so
/// the file is always one writer's whole content; callers that read-modify-write still need
/// a lock to not lose each other's changes (`App`'s write lock).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = tmp_path(path);
    let mut file = create_tmp(&tmp);
    for _ in 1..TMP_TRIES {
        match &file {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                tmp = tmp_path(path);
                file = create_tmp(&tmp);
            }
            _ => break,
        }
    }
    let Ok(mut f) = file else {
        return file.map(|_| ()); // never created: nothing of ours to remove
    };
    let written = f.write_all(bytes).and_then(|()| f.sync_all());
    drop(f);
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp); // a unique name would otherwise linger
        return Err(e);
    }
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
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            1,
            "no tmp left"
        );
        let pid = std::process::id();
        let t = tmp_path(&p);
        let name = t.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(&format!("a.json.{pid}-")) && name.ends_with(".tmp"));
        assert_ne!(tmp_path(&p), t, "unique per call");
        assert_eq!(t.parent(), Some(d.path()));
        for yes in ["a.json.tmp", "a.json.12-3.tmp"] {
            assert!(is_tmp_of(yes, "a.json"), "{yes}");
        }
        for no in [
            "a.json",
            "a.jsontmp",
            "a.json.x-1.tmp",
            "a.json.1-.tmp",
            "a.json.1.tmp",
            "b.json.tmp",
            "a.json.1-2tmp",
        ] {
            assert!(!is_tmp_of(no, "a.json"), "{no}");
        }
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
    fn concurrent_writers_never_corrupt() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("a.json"));
        write_atomic(&p, b"[]").unwrap();
        let writers: Vec<_> = (0..8u8)
            .map(|n| {
                let p = p.clone();
                std::thread::spawn(move || {
                    let body = serde_json::to_vec(&vec![n; 1_000_000]).unwrap();
                    for _ in 0..4 {
                        write_atomic(&p, &body).unwrap();
                    }
                })
            })
            .collect();
        let reader = {
            let p = p.clone();
            std::thread::spawn(move || {
                for _ in 0..50 {
                    let b = std::fs::read(&*p).unwrap();
                    serde_json::from_slice::<Vec<u8>>(&b).expect("a whole file");
                }
            })
        };
        for w in writers {
            w.join().expect("every write succeeds");
        }
        reader.join().unwrap();
        let v: Vec<u8> = serde_json::from_slice(&std::fs::read(&*p).unwrap()).unwrap();
        assert_eq!(v.len(), 1_000_000);
        assert!(
            v.iter().all(|x| *x == v[0]),
            "one writer's payload, not a mix"
        );
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(left.len(), 1, "no tmp files left behind");
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_tmp_link_is_never_written_through() {
        // N2: a symlink at the tmp path must not let the write truncate what it points to.
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        std::fs::write(&victim, b"keep me").unwrap();
        let p = d.path().join("a.json");
        let t = tmp_path(&p);
        std::os::unix::fs::symlink(&victim, &t).unwrap();
        assert_eq!(
            create_tmp(&t).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        // write_atomic's next tmp name, planted too: it moves on to a fresh one.
        let next = {
            let mut s = p.as_os_str().to_owned();
            let n = NEXT_TMP.load(Ordering::Relaxed);
            s.push(format!(".{}-{n}.tmp", std::process::id()));
            PathBuf::from(s)
        };
        std::os::unix::fs::symlink(&victim, &next).unwrap();
        write_atomic(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
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
        // The rename fails (a directory is in the way): the tmp is removed.
        std::fs::create_dir(d.path().join("dir")).unwrap();
        std::fs::write(d.path().join("dir/x"), b"x").unwrap();
        assert!(write_atomic(&d.path().join("dir"), b"x").is_err());
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    }
}
