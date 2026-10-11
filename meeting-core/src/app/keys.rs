//! F24: where `App` reads keys (Meetings' own folder, then SN-Copilot's) and the Settings
//! import. Replies carry paths and provider ids only, never a key (INV2).
use super::App;
use crate::api::error_json;
use crate::keyfile::{self, KeyError, Provider, ProviderKey};
use crate::keyimport::{self, Outcome};
use crate::settings::{self, KeyStatus};
use serde_json::json;
use std::path::{Path, PathBuf};

impl App {
    /// F24-FR2: the imported keys first, then SN-Copilot's folder.
    fn key_dirs(&self) -> [PathBuf; 2] {
        [
            self.storage_root.join(keyimport::IMPORT_DIR),
            self.core.key_dir.clone(),
        ]
    }

    pub(crate) fn load_key(&self, p: Provider) -> Result<ProviderKey, KeyError> {
        let [own, shared] = self.key_dirs();
        keyfile::load_first(&[&own, &shared], p)
    }

    pub(crate) fn key_status(&self, p: Provider) -> KeyStatus {
        settings::key_status_of(p, &self.load_key(p))
    }

    /// `nativeKeyFiles`: the key files Settings offers to import, newest first.
    pub fn key_files_json(&self) -> String {
        let files: Vec<_> = keyimport::find(&self.storage_root)
            .into_iter()
            .map(|f| json!({"path": f.path, "name": f.name, "folder": f.folder}))
            .collect();
        json!({ "files": files }).to_string()
    }

    /// `nativeImportKey(path, replace)`: `{"imported": "openai", "replaced": false}`, or
    /// `{"exists": "openai"}` when Meetings has that key already and `replace` is off.
    pub fn import_key_json(&self, path: &str, replace: bool) -> String {
        let _w = self.write.lock().unwrap_or_else(|e| e.into_inner());
        match keyimport::import(&self.storage_root, Path::new(path), replace) {
            Ok(Outcome::Imported { provider, replaced }) => {
                json!({"imported": provider.id(), "replaced": replaced}).to_string()
            }
            Ok(Outcome::Exists(provider)) => json!({"exists": provider.id()}).to_string(),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// `nativeDeleteKeySource(path)`: delete the copy just imported.
    pub fn delete_key_source_json(&self, path: &str) -> String {
        match keyimport::delete_source(&self.storage_root, Path::new(path)) {
            Ok(()) => json!({"ok": true}).to_string(),
            Err(e) => error_json(&e.to_string()),
        }
    }
}
