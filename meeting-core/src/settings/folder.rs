//! F10-FR3: the save folder must resolve inside internal storage; earlier ones are remembered.
use super::{Settings, SettingsError, err};
use std::path::{Component, Path, PathBuf};

/// Folders under the storage root that must never receive meeting files (F10-FR3), lowercase.
const FORBIDDEN: [&[&str]; 2] = [
    &["mystyle", "sncopilot"],
    &["mystyle", "meetings", "templates"],
];
/// F10-AC4's exact text.
pub const OUTSIDE: &str = "must be inside internal storage";

/// The previous folders after moving to `new`: the old current folder first, then the older
/// ones, without `new` itself and without repeats. Shared storage ignores case, so do we.
pub(super) fn remember(current: &Settings, new: &str) -> Vec<String> {
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for f in std::iter::once(&current.save_folder).chain(&current.previous_folders) {
        if !same(f, new) && !out.iter().any(|o| same(o, f)) {
            out.push(f.clone());
        }
    }
    out
}

/// F10-FR3: `input` relative to `root` (or absolute under it) → the normalised relative path.
/// The folder must resolve inside `root`, also through symlinks, must not be `root` itself or
/// inside MyStyle/SnCopilot or MyStyle/Meetings/templates, and must be creatable and
/// writable. It is created here.
pub fn validate_save_folder(root: &Path, input: &str) -> Result<String, SettingsError> {
    const F: &str = "saveFolder";
    let outside = || err(F, OUTSIDE);
    let trimmed = input.trim();
    let root_s = root.to_string_lossy();
    let rel = match trimmed.strip_prefix(root_s.as_ref()) {
        Some(r) if r.is_empty() || r.starts_with('/') => r,
        _ if trimmed.starts_with('/') => return Err(outside()),
        _ => trimmed,
    };
    if rel.chars().any(|c| c.is_control() || c == '\\') {
        return Err(err(F, "folder name has invalid characters"));
    }
    let mut parts: Vec<&str> = Vec::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str().ok_or_else(outside)?),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => return Err(outside()),
        }
    }
    if parts.is_empty() {
        return Err(err(
            F,
            "choose a folder inside internal storage, not its top level",
        ));
    }
    check_forbidden(&parts)?;
    let canon_root = root
        .canonicalize()
        .map_err(|_| err(F, "internal storage is not available"))?;
    let inside = |p: &Path| p.canonicalize().is_ok_and(|c| c.starts_with(&canon_root));
    // Symlinks: the deepest part that exists must already resolve inside the root.
    let full = parts.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    let mut probe = full.as_path();
    while !probe.exists() {
        probe = probe.parent().ok_or_else(outside)?;
    }
    if !inside(probe) {
        return Err(outside());
    }
    std::fs::create_dir_all(&full)
        .map_err(|e| err(F, format!("can't create the folder: {}", e.kind())))?;
    // ... and so must the folder itself, now that it exists (a link to a forbidden folder).
    let canon = full.canonicalize().map_err(|_| outside())?;
    let canon_parts: Vec<&str> = match canon.strip_prefix(&canon_root) {
        Ok(r) => r.iter().filter_map(|c| c.to_str()).collect(),
        Err(_) => return Err(outside()),
    };
    if canon_parts.is_empty() {
        return Err(outside());
    }
    check_forbidden(&canon_parts)?;
    let test = full.join(".sn-meeting-write-test");
    std::fs::File::create(&test)
        .and_then(|_| std::fs::remove_file(&test))
        .map_err(|e| err(F, format!("the folder isn't writable: {}", e.kind())))?;
    Ok(parts.join("/"))
}

/// Case-insensitive, as shared storage is.
fn check_forbidden(parts: &[&str]) -> Result<(), SettingsError> {
    let lower: Vec<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    for f in FORBIDDEN {
        if lower.len() >= f.len() && lower.iter().zip(f).all(|(a, b)| a == b) {
            return Err(err(
                "saveFolder",
                "can't save into MyStyle/SnCopilot or MyStyle/Meetings/templates",
            ));
        }
    }
    Ok(())
}

impl Settings {
    /// Every library folder, current first, as absolute paths under `root` (F10-FR3).
    pub fn folders(&self, root: &Path) -> Vec<PathBuf> {
        std::iter::once(&self.save_folder)
            .chain(&self.previous_folders)
            .map(|f| root.join(f))
            .collect()
    }
}
