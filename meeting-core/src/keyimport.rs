//! F24: import a `copilot-key-<provider>.txt` that was copied anywhere the user can reach on a
//! tablet (Download, Documents, …) into Meetings' own key folder, so no tablet needs a
//! hand-made `MyStyle/SnCopilot`. The core reads, checks and writes the file itself: only its
//! path crosses JNI, never the key (INV2), and SN-Copilot's folder is never written (INV4).
use crate::keyfile::{self, KeyError, Provider};
use std::path::{Path, PathBuf};

/// Meetings' own key folder under shared storage. Read before SN-Copilot's (F24-FR2).
pub const IMPORT_DIR: &str = "MyStyle/Meetings/keys";
/// Where a copied key file is looked for, a few folders deep; files at the top of shared
/// storage count too. `Document` is the Supernote's folder, `Documents` everyone else's.
pub const ROOTS: [&str; 6] = [
    "Download",
    "Downloads",
    "Documents",
    "Document",
    "INBOX",
    "EXPORT",
];
const MAX_DEPTH: usize = 3;
const LIMIT: usize = 50;

/// A key file found in shared storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub name: String,
    /// Its folder relative to shared storage ("" at the top).
    pub folder: String,
}

/// What an import did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Imported {
        provider: Provider,
        replaced: bool,
    },
    /// Meetings already has a key for this provider; nothing was written.
    Exists(Provider),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("pick a copilot-key file from Download, Documents, INBOX or EXPORT")]
    Refused,
    #[error("{0}")]
    Key(#[from] KeyError),
    #[error("that file hasn't been imported, so it was kept")]
    NotImported,
    #[error("couldn't save the key: {0}")]
    Io(String),
}

/// Whether `name` looks like a key file: `copilot-key…txt`, any case, so a browser's
/// "copilot-key-openai (1).txt" counts.
pub fn is_key_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower.starts_with("copilot-key") && lower.ends_with(".txt")
}

/// F24-FR3: every key file under `storage`'s [`ROOTS`] and at its top, newest first. Links
/// are not followed and dot folders are skipped.
pub fn find(storage: &Path) -> Vec<Found> {
    let mut out: Vec<(std::time::SystemTime, Found)> = Vec::new();
    let mut add = |path: PathBuf| {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !meta.is_file() || !is_key_name(&name) {
            return;
        }
        let folder = path
            .parent()
            .and_then(|p| p.strip_prefix(storage).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let when = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        out.push((when, Found { path, name, folder }));
    };
    fn walk(dir: &Path, depth: usize, add: &mut dyn FnMut(PathBuf)) {
        let Ok(kids) = std::fs::read_dir(dir) else {
            return;
        };
        for kid in kids.filter_map(Result::ok) {
            let Ok(kind) = kid.file_type() else { continue };
            let hidden = kid.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() && !hidden && depth < MAX_DEPTH {
                walk(&kid.path(), depth + 1, add);
            } else if kind.is_file() {
                add(kid.path());
            }
        }
    }
    if let Ok(top) = std::fs::read_dir(storage) {
        top.filter_map(Result::ok).for_each(|e| add(e.path()));
    }
    for root in ROOTS {
        walk(&storage.join(root), 1, &mut add);
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    out.into_iter().map(|(_, f)| f).take(LIMIT).collect()
}

/// A file the user may import or delete: a real file (not a link) named like a key file,
/// inside shared storage but outside `MyStyle` (the key folders themselves).
fn source(storage: &Path, path: &Path) -> Result<PathBuf, ImportError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ImportError::Refused)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !meta.is_file() || !is_key_name(&name) {
        return Err(ImportError::Refused);
    }
    let real = path.canonicalize().map_err(|_| ImportError::Refused)?;
    let root = storage.canonicalize().map_err(|_| ImportError::Refused)?;
    if !real.starts_with(&root) || real.starts_with(root.join("MyStyle")) {
        return Err(ImportError::Refused);
    }
    Ok(real)
}

/// Read and check a key file: it must name a provider and hold its key and model.
fn read_key(path: &Path) -> Result<(Provider, String), ImportError> {
    let text = keyfile::read_capped(path)?;
    let provider = keyfile::provider_of(&text)?;
    keyfile::parse(&text, provider)?;
    Ok((provider, text))
}

/// Where Meetings keeps `provider`'s imported key.
pub fn target(storage: &Path, provider: Provider) -> PathBuf {
    storage
        .join(IMPORT_DIR)
        .join(provider.canonical_file_name())
}

/// F24-FR4: copy a checked key file into [`IMPORT_DIR`] as `copilot-key-<provider>.txt`,
/// whatever it was called. An existing key is replaced only when `replace` is set.
pub fn import(storage: &Path, path: &Path, replace: bool) -> Result<Outcome, ImportError> {
    let src = source(storage, path)?;
    let (provider, text) = read_key(&src)?;
    let to = target(storage, provider);
    let existed = std::fs::symlink_metadata(&to).is_ok();
    if existed && !replace {
        return Ok(Outcome::Exists(provider));
    }
    let io = |e: std::io::Error| ImportError::Io(e.kind().to_string());
    std::fs::create_dir_all(to.parent().expect("IMPORT_DIR has a parent")).map_err(io)?;
    crate::atomic::write_atomic(&to, text.as_bytes()).map_err(io)?;
    Ok(Outcome::Imported {
        provider,
        replaced: existed,
    })
}

/// F24-FR5: delete the copy the user imported from, only once Meetings' own file holds the
/// same text (verify, then delete; anything else keeps the file).
pub fn delete_source(storage: &Path, path: &Path) -> Result<(), ImportError> {
    let src = source(storage, path)?;
    let (provider, text) = read_key(&src)?;
    let kept =
        keyfile::read_capped(&target(storage, provider)).map_err(|_| ImportError::NotImported)?;
    if kept != text {
        return Err(ImportError::NotImported);
    }
    std::fs::remove_file(&src).map_err(|e| ImportError::Io(e.kind().to_string()))
}
