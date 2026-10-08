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

/// The path's names, checked without touching the filesystem: no `..`, no control characters
/// or backslashes, not the top level, not a forbidden folder by name.
fn relative_parts(rel: &str) -> Result<Vec<&str>, SettingsError> {
    const F: &str = "saveFolder";
    let outside = || err(F, OUTSIDE);
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
    Ok(parts)
}

/// Whether a stored (relative) save folder could have passed `validate_save_folder`; used
/// when loading settings.json, which may have been edited by hand.
pub(super) fn plausible(rel: &str) -> bool {
    !rel.starts_with('/') && relative_parts(rel).is_ok_and(|p| p.join("/") == rel)
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
    let parts = relative_parts(rel)?;
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
    // Before creating anything: where the folder will really be is the existing part, resolved
    // through any links, plus the parts still to create (a link Document → MyStyle/SnCopilot
    // must not get MyStyle/SnCopilot/Meetings made).
    if aliases_forbidden(root, probe) {
        return Err(forbidden());
    }
    let existing = probe.canonicalize().map_err(|_| outside())?;
    let rest = full.strip_prefix(probe).map_err(|_| outside())?;
    let target = existing.join(rest);
    let target_parts: Vec<&str> = match target.strip_prefix(&canon_root) {
        Ok(r) => r.iter().filter_map(|c| c.to_str()).collect(),
        Err(_) => return Err(outside()),
    };
    check_forbidden(&target_parts)?;
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

/// Lowercase, with every letter whose upper or lower case is ASCII replaced by that ASCII.
fn fold(name: &str) -> String {
    let ascii = |s: String| s.is_ascii().then(|| s.to_ascii_lowercase());
    name.chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_ascii_lowercase().to_string()
            } else {
                ascii(c.to_uppercase().collect())
                    .or_else(|| ascii(c.to_lowercase().collect()))
                    .unwrap_or_else(|| c.to_lowercase().collect())
            }
        })
        .collect()
}

fn forbidden() -> SettingsError {
    err(
        "saveFolder",
        "can't save into MyStyle/SnCopilot or MyStyle/Meetings/templates",
    )
}

/// Case-insensitive, as shared storage is. Android's storage folds case with full Unicode case
/// folding, under which some non-ASCII letters equal ASCII ones ("ſ" is "s", the Kelvin sign
/// is "k"), so those are folded too before comparing (DEVICE-UNVERIFIED on the device).
fn check_forbidden(parts: &[&str]) -> Result<(), SettingsError> {
    let lower: Vec<String> = parts.iter().map(|p| fold(p)).collect();
    for f in FORBIDDEN {
        if lower.len() >= f.len() && lower.iter().zip(f).all(|(a, b)| a == b) {
            return Err(forbidden());
        }
    }
    Ok(())
}

/// Whether `existing` or any folder above it (up to `root`) is a forbidden folder under
/// another name: same device and inode, whatever the spelling, case folding or links.
/// DEVICE-UNVERIFIED: Android's casefolded shared storage is not reproducible on the host.
#[cfg(unix)]
fn aliases_forbidden(root: &Path, existing: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let id = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
    let names = [
        ["MyStyle", "SnCopilot"].as_slice(),
        &["MyStyle", "Meetings", "templates"],
    ];
    let bad: Vec<(u64, u64)> = names
        .iter()
        .filter_map(|f| id(&f.iter().fold(root.to_path_buf(), |p, c| p.join(c))))
        .collect();
    !bad.is_empty()
        && existing
            .ancestors()
            .take_while(|a| a.starts_with(root) && *a != root)
            .any(|a| id(a).is_some_and(|x| bad.contains(&x)))
}

#[cfg(not(unix))]
fn aliases_forbidden(_: &Path, _: &Path) -> bool {
    false
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
