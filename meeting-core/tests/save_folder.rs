//! F10-FR3: the save folder must resolve inside internal storage, be creatable and writable,
//! and stay out of sn-copilot's and the templates' folders. Earlier folders are remembered.
use meeting::api::Core;
use meeting::app::App;
use meeting::settings::{OUTSIDE, validate_save_folder};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn check(root: &Path, input: &str) -> Result<String, String> {
    validate_save_folder(root, input).map_err(|e| {
        assert_eq!(e.field, Some("saveFolder"));
        e.message
    })
}

#[test]
fn the_f10_ac4_path_is_rejected_with_its_text() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(check(d.path(), "../../data"), Err(OUTSIDE.into()));
    assert_eq!(OUTSIDE, "must be inside internal storage");
}

#[test]
fn relative_and_absolute_forms_normalise() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    for (input, want) in [
        ("Document/Meetings", "Document/Meetings"),
        ("  Document/Meetings/  ", "Document/Meetings"),
        ("./EXPORT//sn-meeting", "EXPORT/sn-meeting"),
        ("Notes – 2026/Réunions", "Notes – 2026/Réunions"),
    ] {
        assert_eq!(check(root, input), Ok(want.into()), "{input:?}");
        assert!(root.join(want).is_dir(), "{want} created");
    }
    let abs = format!("{}/EXPORT/abs", root.display());
    assert_eq!(check(root, &abs), Ok("EXPORT/abs".into()));
}

#[test]
fn escapes_and_odd_names_are_rejected() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    for bad in [
        "/data/x",
        "a/../../b",
        "a/../b",
        "..",
        "/storage/emulated/0x/y",
    ] {
        assert_eq!(check(root, bad), Err(OUTSIDE.into()), "{bad:?}");
    }
    let lookalike = format!("{}x/y", root.display());
    assert_eq!(check(root, &lookalike), Err(OUTSIDE.into()));
    let root_slash = format!("{}/", root.display());
    for top in [
        "",
        "  ",
        ".",
        "./",
        &root.display().to_string(),
        &root_slash,
    ] {
        assert_eq!(
            check(root, top),
            Err("choose a folder inside internal storage, not its top level".into()),
            "{top:?}"
        );
    }
    for bad in ["a\\b", "tab\there", "nul\0x", "new\nline"] {
        assert_eq!(
            check(root, bad),
            Err("folder name has invalid characters".into()),
            "{bad:?}"
        );
    }
}

#[test]
fn sn_copilot_and_templates_folders_are_off_limits() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let msg = "can't save into MyStyle/SnCopilot or MyStyle/Meetings/templates";
    for bad in [
        "MyStyle/SnCopilot",
        "mystyle/sncopilot/sub",
        "MYSTYLE/SNCOPILOT",
        "MyStyle/Meetings/templates",
        "MyStyle/meetings/Templates/x",
    ] {
        assert_eq!(check(root, bad), Err(msg.into()), "{bad}");
    }
    for ok in [
        "MyStyle/Meetings",
        "MyStyle/SnCopilotX",
        "MyStyle/Meetings/templates-old",
    ] {
        assert!(check(root, ok).is_ok(), "{ok}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_or_reach_a_forbidden_folder() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), root.join("link")).unwrap();
    assert_eq!(check(root, "link/x"), Err(OUTSIDE.into()));
    assert_eq!(check(root, "link"), Err(OUTSIDE.into()));
    assert!(
        !elsewhere.path().join("x").exists(),
        "nothing created outside"
    );
    fs::create_dir_all(root.join("MyStyle/SnCopilot")).unwrap();
    symlink(root.join("MyStyle/SnCopilot"), root.join("keys")).unwrap();
    assert_eq!(
        check(root, "keys"),
        Err("can't save into MyStyle/SnCopilot or MyStyle/Meetings/templates".into())
    );
    symlink(root, root.join("top")).unwrap();
    assert_eq!(check(root, "top"), Err(OUTSIDE.into()));
    // A link that stays inside is fine.
    fs::create_dir_all(root.join("Document")).unwrap();
    symlink(root.join("Document"), root.join("docs")).unwrap();
    assert_eq!(check(root, "docs/Meetings"), Ok("docs/Meetings".into()));
}

#[cfg(unix)]
#[test]
fn the_folder_must_be_creatable_and_writable() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    fs::create_dir(root.join("locked")).unwrap();
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o500)).unwrap();
    let create = check(root, "locked/new");
    let write = check(root, "locked");
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        create,
        Err("can't create the folder: permission denied".into())
    );
    assert_eq!(
        write,
        Err("the folder isn't writable: permission denied".into())
    );
    // A leftover probe file from an interrupted check doesn't count as "not writable".
    fs::write(root.join("locked/.sn-meeting-write-test"), "").unwrap();
    assert_eq!(check(root, "locked"), Ok("locked".into()));
    assert!(!root.join("locked/.sn-meeting-write-test").exists());
}

#[test]
fn a_missing_storage_root_is_reported() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        check(&d.path().join("gone"), "Document/Meetings"),
        Err("internal storage is not available".into())
    );
}

fn app(data: &Path, root: &Path) -> App {
    let mut core = Core::device(data.to_str().unwrap());
    core.key_dir = data.join("no-keys");
    App::new(core, root.into())
}

fn set(app: &App, patch: Value) -> Value {
    serde_json::from_str(&app.set_settings_json(&patch.to_string())).unwrap()
}

#[test]
fn moving_folders_remembers_the_old_ones_newest_first() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = app(data.path(), root.path());
    let s = set(&app, json!({"saveFolder": "Document/Meetings"}));
    assert_eq!(s["settings"]["saveFolder"], "Document/Meetings");
    assert_eq!(
        s["settings"]["previousFolders"],
        json!(["EXPORT/sn-meeting"])
    );
    assert_eq!(
        s["effective"]["saveFolderPath"],
        json!(root.path().join("Document/Meetings"))
    );
    let s = set(&app, json!({"saveFolder": "Custom/A"}));
    assert_eq!(
        s["settings"]["previousFolders"],
        json!(["Document/Meetings", "EXPORT/sn-meeting"])
    );
    // Back to an earlier folder: it leaves the list, nothing repeats, case is ignored.
    let s = set(&app, json!({"saveFolder": "export/SN-meeting"}));
    assert_eq!(s["settings"]["saveFolder"], "export/SN-meeting");
    assert_eq!(
        s["settings"]["previousFolders"],
        json!(["Custom/A", "Document/Meetings"])
    );
    // A rejected folder changes nothing.
    assert_eq!(
        set(&app, json!({"saveFolder": "../../data"})),
        json!({"error": OUTSIDE, "field": "saveFolder"})
    );
    let s: Value = serde_json::from_str(&app.get_settings_json()).unwrap();
    assert_eq!(s["settings"]["saveFolder"], "export/SN-meeting");
    // Folders of the new settings, current first.
    let (settings, _) = app.settings();
    assert_eq!(
        settings.folders(root.path()),
        ["export/SN-meeting", "Custom/A", "Document/Meetings"].map(|f| root.path().join(f))
    );
}
