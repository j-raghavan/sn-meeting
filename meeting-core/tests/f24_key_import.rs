//! F24: importing a key file from Download, Documents, … into Meetings' own key folder, and
//! reading that folder before SN-Copilot's. Every reply is checked for the fixture keys (INV2),
//! and SN-Copilot's folder is never written (INV4).
mod common;

use common::*;
use meeting::api::Core;
use meeting::app::App;
use meeting::keyfile::{self, KeyError, Provider};
use meeting::keyimport::{self, Outcome};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

struct Env {
    keys: tempfile::TempDir,
    data: tempfile::TempDir,
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            keys: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn app(&self) -> App {
        let mut core = Core::device(self.data.path().to_str().unwrap());
        core.key_dir = self.keys.path().into();
        App::new(core, self.root.path().into())
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    /// A file at `rel` under shared storage.
    fn put(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.root().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        p
    }
    fn imported(&self, p: Provider) -> PathBuf {
        keyimport::target(self.root(), p)
    }
}

fn openai(model: &str) -> String {
    format!("provider=openai\nkey={OA}\nmodel={model}\n")
}

fn reply(s: String) -> Value {
    assert_no_key(&s);
    serde_json::from_str(&s).unwrap()
}

#[test]
fn key_names_are_matched_loosely() {
    for ok in [
        "copilot-key-openai.txt",
        "Copilot-Key-Gemini.TXT",
        "copilot-key-openai (1).txt",
        " copilot-key.txt ",
    ] {
        assert!(keyimport::is_key_name(ok), "{ok}");
    }
    for no in [
        "openai.txt",
        "copilot-key-openai.txt.bak",
        "key.txt",
        "copilot-key-openai",
    ] {
        assert!(!keyimport::is_key_name(no), "{no}");
    }
}

#[test]
fn provider_of_reads_the_provider_line_only() {
    assert_eq!(keyfile::provider_of(&openai("m")), Ok(Provider::OpenAi));
    assert_eq!(
        keyfile::provider_of("# c\r\nprovider = gemini \r\nkey=x"),
        Ok(Provider::Gemini)
    );
    assert_eq!(
        keyfile::provider_of("key=x\nmodel=m\n"),
        Err(KeyError::MissingField("provider"))
    );
    assert_eq!(
        keyfile::provider_of("provider=\nkey=x"),
        Err(KeyError::MissingField("provider"))
    );
    assert_eq!(
        keyfile::provider_of("provider=deepseek\nkey=x"),
        Err(KeyError::UnknownProvider("deepseek".into()))
    );
    // An unknown value may be the key itself: it is never echoed.
    let e = keyfile::provider_of(&format!("provider={OA}")).unwrap_err();
    assert_eq!(e, KeyError::UnknownProvider("<invalid>".into()));
    assert!(!e.to_string().contains(OA));
    assert_eq!(
        e.to_string(),
        "provider=\"<invalid>\" isn't openai, gemini or anthropic"
    );
}

#[test]
fn find_lists_key_files_in_the_usual_folders_newest_first() {
    let e = Env::new();
    let old = e.put("Download/copilot-key-openai.txt", &openai("m"));
    let filetime = |p: &Path, secs: u64| {
        let f = fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    };
    filetime(&old, 1_000);
    let new = e.put(
        "Documents/keys/deep/copilot-key-gemini.txt",
        "provider=gemini",
    );
    filetime(&new, 2_000);
    let top = e.put("copilot-key-anthropic.txt", "provider=anthropic");
    filetime(&top, 1_500);
    // Not offered: other names, too deep, dot folders, other roots, and the key folders.
    e.put("Download/notes.txt", "x");
    e.put("Download/a/b/c/copilot-key-openai.txt", "x");
    e.put("Download/.hidden/copilot-key-openai.txt", "x");
    e.put("Pictures/copilot-key-openai.txt", "x");
    e.put("MyStyle/SnCopilot/copilot-key-openai.txt", "x");
    e.put("MyStyle/Meetings/keys/copilot-key-openai.txt", "x");
    let found = keyimport::find(e.root());
    let names: Vec<(&str, &str)> = found
        .iter()
        .map(|f| (f.folder.as_str(), f.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Documents/keys/deep", "copilot-key-gemini.txt"),
            ("", "copilot-key-anthropic.txt"),
            ("Download", "copilot-key-openai.txt"),
        ]
    );
    let j = reply(e.app().key_files_json());
    assert_eq!(j["files"].as_array().unwrap().len(), 3);
    assert_eq!(j["files"][2]["name"], "copilot-key-openai.txt");
    assert_eq!(j["files"][2]["folder"], "Download");
    assert_eq!(j["files"][2]["path"], json!(old));
}

#[test]
fn find_does_not_follow_links_and_copes_with_no_storage() {
    let e = Env::new();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("copilot-key-openai.txt"), openai("m")).unwrap();
    fs::create_dir_all(e.root().join("Download")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), e.root().join("Download/linked")).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.path().join("copilot-key-openai.txt"),
        e.root().join("Download/copilot-key-openai.txt"),
    )
    .unwrap();
    assert!(keyimport::find(e.root()).is_empty());
    assert!(keyimport::find(&e.root().join("missing")).is_empty());
}

#[test]
fn an_import_writes_meetings_own_file_and_is_used_first() {
    let e = Env::new();
    // SN-Copilot's folder has an older key; the imported one must win, and stay untouched.
    fs::write(
        e.keys.path().join("copilot-key-openai.txt"),
        openai("from-sncopilot"),
    )
    .unwrap();
    let src = e.put("Download/copilot-key-openai (1).txt", &openai("imported"));
    let app = e.app();
    assert_eq!(
        reply(app.import_key_json(src.to_str().unwrap(), false)),
        json!({"imported": "openai", "replaced": false})
    );
    assert_eq!(
        fs::read_to_string(e.imported(Provider::OpenAi)).unwrap(),
        openai("imported")
    );
    assert_eq!(
        fs::read_to_string(e.keys.path().join("copilot-key-openai.txt")).unwrap(),
        openai("from-sncopilot")
    );
    let s = reply(app.get_settings_json());
    assert_eq!(s["keys"]["openai"]["status"], "found");
    assert_eq!(s["keys"]["openai"]["model"], "imported");
    assert!(
        src.exists(),
        "the copy is kept until the user says to delete it"
    );
}

#[test]
fn an_existing_key_is_replaced_only_when_asked() {
    let e = Env::new();
    let first = e.put("Download/copilot-key-openai.txt", &openai("one"));
    let app = e.app();
    reply(app.import_key_json(first.to_str().unwrap(), false));
    let second = e.put("Documents/copilot-key-gpt.txt", &openai("two"));
    assert_eq!(
        reply(app.import_key_json(second.to_str().unwrap(), false)),
        json!({"exists": "openai"})
    );
    assert_eq!(
        fs::read_to_string(e.imported(Provider::OpenAi)).unwrap(),
        openai("one")
    );
    assert_eq!(
        reply(app.import_key_json(second.to_str().unwrap(), true)),
        json!({"imported": "openai", "replaced": true})
    );
    assert_eq!(
        fs::read_to_string(e.imported(Provider::OpenAi)).unwrap(),
        openai("two")
    );
}

#[test]
fn the_file_name_does_not_decide_the_provider() {
    let e = Env::new();
    let src = e.put(
        "INBOX/copilot-key-openai.txt",
        &format!("provider=gemini\nkey={GE}\nmodel=gemini-z\n"),
    );
    assert_eq!(
        keyimport::import(e.root(), &src, false),
        Ok(Outcome::Imported {
            provider: Provider::Gemini,
            replaced: false
        })
    );
    assert!(e.imported(Provider::Gemini).is_file());
    assert!(!e.imported(Provider::OpenAi).exists());
}

#[test]
fn a_file_that_is_not_a_usable_key_is_refused_with_its_reason() {
    let e = Env::new();
    let app = e.app();
    let cases = [
        ("key=x\nmodel=m\n", "missing required field: provider"),
        ("provider=openai\nmodel=m\n", "missing required field: key"),
        (
            &format!("provider=openai\nkey={OA}\n") as &str,
            "missing required field: model",
        ),
        (
            "provider=deepseek\nkey=k\nmodel=m\n",
            "provider=\"deepseek\" isn't openai, gemini or anthropic",
        ),
    ];
    for (body, msg) in cases {
        let src = e.put("Download/copilot-key-x.txt", body);
        assert_eq!(
            reply(app.import_key_json(src.to_str().unwrap(), true)),
            json!({"error": msg})
        );
    }
    let big = e.put("Download/copilot-key-big.txt", &"#".repeat(70_000));
    assert_eq!(
        reply(app.import_key_json(big.to_str().unwrap(), true)),
        json!({"error": "key file unreadable: file too large"})
    );
    assert!(!e.root().join(keyimport::IMPORT_DIR).exists());
}

#[test]
fn only_key_files_inside_shared_storage_and_outside_mystyle_are_taken() {
    let e = Env::new();
    let refused =
        json!({"error": "pick a copilot-key file from Download, Documents, INBOX or EXPORT"});
    let app = e.app();
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("copilot-key-openai.txt");
    fs::write(&outside, openai("m")).unwrap();
    let link = e.root().join("Download/copilot-key-link.txt");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let paths = [
        outside.clone(),
        link,
        e.put("Download/openai.txt", &openai("m")),
        e.put("MyStyle/SnCopilot/copilot-key-openai.txt", &openai("m")),
        e.root().join("Download/copilot-key-missing.txt"),
        e.root().join("Download"),
    ];
    for p in paths {
        assert_eq!(
            reply(app.import_key_json(p.to_str().unwrap(), true)),
            refused,
            "{p:?}"
        );
        assert_eq!(
            reply(app.delete_key_source_json(p.to_str().unwrap())),
            refused,
            "{p:?}"
        );
    }
    assert!(outside.exists());
    assert!(!e.root().join(keyimport::IMPORT_DIR).exists());
}

#[test]
fn the_copy_is_deleted_only_after_a_matching_import() {
    let e = Env::new();
    let app = e.app();
    let src = e.put("Download/copilot-key-openai.txt", &openai("m"));
    let not_imported = json!({"error": "that file hasn't been imported, so it was kept"});
    assert_eq!(
        reply(app.delete_key_source_json(src.to_str().unwrap())),
        not_imported
    );
    assert!(src.exists());
    reply(app.import_key_json(src.to_str().unwrap(), false));
    // A different file for the same provider is not the one Meetings holds: kept.
    let other = e.put("Documents/copilot-key-openai.txt", &openai("other"));
    assert_eq!(
        reply(app.delete_key_source_json(other.to_str().unwrap())),
        not_imported
    );
    assert!(other.exists());
    let broken = e.put("Documents/copilot-key-x.txt", "provider=openai\n");
    assert_eq!(
        reply(app.delete_key_source_json(broken.to_str().unwrap())),
        json!({"error": "missing required field: key"})
    );
    assert_eq!(
        reply(app.delete_key_source_json(src.to_str().unwrap())),
        json!({"ok": true})
    );
    assert!(!src.exists());
    assert!(e.imported(Provider::OpenAi).is_file());
}

#[test]
fn an_unwritable_key_folder_is_reported_without_the_key() {
    let e = Env::new();
    // A file where the folder should be.
    e.put("MyStyle/Meetings/keys", "not a folder");
    let src = e.put("Download/copilot-key-openai.txt", &openai("m"));
    let r = reply(e.app().import_key_json(src.to_str().unwrap(), false));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .starts_with("couldn't save the key: "),
        "{r}"
    );
}

#[test]
fn load_first_prefers_a_good_key_and_reports_the_first_real_error() {
    let own = tempfile::tempdir().unwrap();
    let shared = tempfile::tempdir().unwrap();
    let dirs = [own.path(), shared.path()];
    let none = keyfile::load_first(&dirs, Provider::OpenAi).unwrap_err();
    assert_eq!(
        none,
        KeyError::NotFound {
            provider: "openai",
            vault_present: false
        }
    );
    fs::write(shared.path().join("copilot-key.enc"), "v").unwrap();
    assert_eq!(
        keyfile::load_first(&dirs, Provider::OpenAi).unwrap_err(),
        KeyError::NotFound {
            provider: "openai",
            vault_present: true
        }
    );
    // A broken imported file gives way to a good one in SN-Copilot's folder.
    fs::write(
        own.path().join("copilot-key-openai.txt"),
        "provider=openai\n",
    )
    .unwrap();
    assert_eq!(
        keyfile::load_first(&dirs, Provider::OpenAi).unwrap_err(),
        KeyError::MissingField("key")
    );
    fs::write(
        shared.path().join("copilot-key-openai.txt"),
        openai("shared"),
    )
    .unwrap();
    assert_eq!(
        keyfile::load_first(&dirs, Provider::OpenAi).unwrap().model,
        "shared"
    );
    fs::write(own.path().join("copilot-key-openai.txt"), openai("own")).unwrap();
    let k = keyfile::load_first(&dirs, Provider::OpenAi).unwrap();
    assert_eq!(k.model, "own");
    assert_eq!(k.source, own.path().join("copilot-key-openai.txt"));
}

#[test]
fn the_missing_key_hint_is_unchanged() {
    let e = Env::new();
    let s = reply(e.app().get_settings_json());
    assert_eq!(
        s["keys"]["gemini"]["detail"],
        "Add copilot-key-gemini.txt to MyStyle/SnCopilot"
    );
}
