//! F11-FR3: user copies in `MyStyle/Meetings/templates/<id>.md` and their precedence over the
//! built-ins. The folder is rescanned on every call; a fingerprint of (name, mtime, size) keeps
//! the parsed set until a file changes. No folder, or an unreadable one, gives the six built-ins
//! (ADR D10).
use super::{BUILTINS, Template, TemplateError, builtins, parse};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

/// Under the storage root.
pub const USER_DIR: &str = "MyStyle/Meetings/templates";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Builtin,
    /// A valid user copy of a built-in.
    Customised,
    /// A user template with an id of its own.
    Yours,
}

/// One row of `nativeListTemplates`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub template: Template,
    pub origin: Origin,
    /// Why the user's file is not in use ("Your copy has an error on line N; using the
    /// built-in"). A YOURS template with a problem can't be used.
    pub problem: Option<String>,
    /// The user's file, when there is one.
    pub path: Option<String>,
}

impl Entry {
    /// Whether a job may use it.
    pub fn usable(&self) -> bool {
        self.problem.is_none() || self.origin != Origin::Yours
    }
}

type Fingerprint = Vec<(String, Option<SystemTime>, u64)>;

/// The templates folder and the last parsed set.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    cache: Mutex<Option<(Fingerprint, Vec<Entry>)>>,
    rebuilds: Mutex<usize>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store {
            dir,
            cache: Mutex::new(None),
            rebuilds: Mutex::new(0),
        }
    }

    /// `<storage root>/MyStyle/Meetings/templates`.
    pub fn under(storage_root: &Path) -> Store {
        Store::new(storage_root.join(USER_DIR))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the user copy of `id` lives (`id` must be a template id: it can't name a path).
    pub fn path_of(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.md"))
    }

    /// How many times the set was re-parsed (tests check the cache).
    pub fn rebuilds(&self) -> usize {
        *self.rebuilds.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every template: the built-ins (a valid user copy in place of its built-in), then the
    /// user's own, by id.
    pub fn list(&self) -> Vec<Entry> {
        let fp = self.fingerprint();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((old, entries)) = cache.as_ref()
            && *old == fp
        {
            return entries.clone();
        }
        let entries = self.scan(&fp);
        *self.rebuilds.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        *cache = Some((fp, entries.clone()));
        entries
    }

    /// The templates a job may use (F11-FR3 effective set).
    pub fn effective(&self) -> Vec<Template> {
        self.list()
            .into_iter()
            .filter(Entry::usable)
            .map(|e| e.template)
            .collect()
    }

    /// The effective entry for `id`.
    pub fn get(&self, id: &str) -> Option<Entry> {
        self.list().into_iter().find(|e| e.template.id == id)
    }

    /// `(file name, mtime, size)` of every `.md` in the folder, sorted.
    fn fingerprint(&self) -> Fingerprint {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut v: Fingerprint = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let m = e.metadata().ok()?;
                (name.ends_with(".md") && m.is_file()).then(|| (name, m.modified().ok(), m.len()))
            })
            .collect();
        v.sort();
        v
    }

    fn scan(&self, fp: &Fingerprint) -> Vec<Entry> {
        let users: Vec<(String, PathBuf, Result<Template, TemplateError>)> = fp
            .iter()
            .filter_map(|(name, _, _)| {
                let stem = name.strip_suffix(".md")?;
                crate::settings::is_template_id(stem).then(|| {
                    let p = self.dir.join(name);
                    (stem.to_owned(), p.clone(), read(&p, stem))
                })
            })
            .collect();
        let shown = |p: &Path| Some(p.to_string_lossy().into_owned());
        let mut out: Vec<Entry> = builtins()
            .into_iter()
            .map(|b| match users.iter().find(|(s, _, _)| *s == b.id) {
                Some((_, p, Ok(t))) => Entry {
                    template: t.clone(),
                    origin: Origin::Customised,
                    problem: None,
                    path: shown(p),
                },
                Some((_, p, Err(e))) => Entry {
                    template: b,
                    origin: Origin::Builtin,
                    problem: Some(problem("Your copy has", e, "using the built-in")),
                    path: shown(p),
                },
                None => Entry {
                    template: b,
                    origin: Origin::Builtin,
                    problem: None,
                    path: None,
                },
            })
            .collect();
        for (stem, p, r) in users {
            if BUILTINS.iter().any(|(id, _)| *id == stem) {
                continue;
            }
            out.push(match r {
                Ok(template) => Entry {
                    template,
                    origin: Origin::Yours,
                    problem: None,
                    path: shown(&p),
                },
                Err(e) => Entry {
                    template: Template {
                        id: stem.clone(),
                        name: stem,
                        ..Template::empty()
                    },
                    origin: Origin::Yours,
                    problem: Some(problem("This file has", &e, "it can't be used")),
                    path: shown(&p),
                },
            });
        }
        out
    }
}

fn problem(what: &str, e: &TemplateError, then: &str) -> String {
    match e.line {
        Some(l) => format!("{what} an error on line {l}; {then}"),
        None => format!("{what} an error ({}); {then}", e.error),
    }
}

/// A user file, parsed; its id must match its file name.
fn read(p: &Path, stem: &str) -> Result<Template, TemplateError> {
    let fail = |error: String| TemplateError {
        error,
        line: None,
        field: None,
    };
    let bytes = crate::atomic::read_capped(p, super::MAX_FILE_BYTES as u64).map_err(|e| {
        let why = match e.kind() {
            std::io::ErrorKind::InvalidData => "the file is larger than 64 KB".to_owned(),
            k => format!("the file can't be read: {k:?}"),
        };
        fail(why)
    })?;
    let text = String::from_utf8(bytes).map_err(|_| fail("the file isn't UTF-8 text".into()))?;
    let t = parse(&text)?;
    if t.id != stem {
        return Err(fail(format!("its id {} doesn't match the file name", t.id)));
    }
    Ok(t)
}
