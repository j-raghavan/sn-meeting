//! The meeting edits the UI makes: to-dos (F6-FR2), the title (F7-FR5), the template
//! (F7-FR2) and deleting a meeting (F7-FR4).
use super::list::Source;
use super::{LibError, Library, Liveness, invalid};
use crate::sidecar::{self, TitleSource};
use serde::Serialize;

/// Longest title, in characters.
const MAX_TITLE: usize = 200;

/// One row of `nativeOpenTodos` (F6-FR1).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoRef {
    pub meeting_id: String,
    pub meeting_started_at: String,
    pub meeting_title: Option<String>,
    pub todo_id: String,
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
}

impl Library<'_> {
    /// F6-FR1: open to-dos from every readable sidecar in every folder, newest meeting first,
    /// so a to-do never disappears because its meeting fell out of the recent five.
    pub fn open_todos(&self) -> Vec<TodoRef> {
        self.all()
            .into_iter()
            .filter(|(m, _)| m.source == Source::Sidecar)
            .flat_map(|(_, sc)| {
                let (id, at, title) = (sc.id, sc.started_at, sc.title);
                sc.todos
                    .into_iter()
                    .filter(|t| !t.done)
                    .map(move |t| TodoRef {
                        meeting_id: id.clone(),
                        meeting_started_at: at.clone(),
                        meeting_title: title.clone(),
                        todo_id: t.id,
                        text: t.text,
                        owner: t.owner,
                        due: t.due,
                    })
            })
            .collect()
    }

    /// F6-FR2: tick or untick one to-do.
    pub fn set_todo_done(&self, id: &str, todo_id: &str, done: bool) -> Result<(), LibError> {
        let (dir, mut sc) = self.load_for_write(id)?;
        let todo = sc
            .todos
            .iter_mut()
            .find(|t| t.id == todo_id)
            .ok_or_else(|| invalid("to-do not found"))?;
        todo.done = done;
        sidecar::write(&dir, &sc)?;
        Ok(())
    }

    /// F7-FR5: a user title (`titleSource = user`, which extraction never overwrites). An
    /// empty or blank title returns the meeting to untitled, so the next extraction may name
    /// it. Legacy audio gets its sidecar here.
    pub fn rename(&self, id: &str, title: &str) -> Result<(), LibError> {
        let t = title.trim();
        if t.chars().any(char::is_control) {
            return Err(invalid("title can't contain control characters"));
        }
        if t.chars().count() > MAX_TITLE {
            return Err(invalid("title is too long"));
        }
        let (dir, mut sc) = self.load_for_write(id)?;
        (sc.title, sc.title_source) = if t.is_empty() {
            (None, None)
        } else {
            (Some(t.to_owned()), Some(TitleSource::User))
        };
        sidecar::write(&dir, &sc)?;
        Ok(())
    }

    /// F7-FR2 / F11-FR6: the template the next Transcribe or Summarise again uses. The id
    /// must be a template id; `App` checks that it is in the effective set.
    pub fn set_template(&self, id: &str, template_id: &str) -> Result<(), LibError> {
        if !crate::settings::is_template_id(template_id) {
            return Err(invalid("invalid template id"));
        }
        let (dir, mut sc) = self.load_for_write(id)?;
        sc.template_id = Some(template_id.to_owned());
        sidecar::write(&dir, &sc)?;
        Ok(())
    }

    /// F7-FR4: remove the meeting's audio, transcript and sidecar (and a stray `.tmp`). Only
    /// names that belong to this exact id are touched, so `…211706b` survives deleting
    /// `…211706`. Refused while it is recording or a job is running on it. Audio goes first and
    /// the sidecar last, so a failure part-way leaves a listed meeting, not hidden audio.
    pub fn delete(&self, id: &str, live: &Liveness) -> Result<Vec<String>, LibError> {
        if live.recording.as_deref() == Some(id) {
            return Err(invalid("meeting is recording"));
        }
        if live.jobs.contains(id) {
            return Err(invalid("meeting is being transcribed"));
        }
        let f = self.find(id)?;
        let mut names: Vec<String> = f.audio.iter().map(|(_, n)| n.clone()).collect();
        if f.has_sidecar
            && let Ok(sc) = sidecar::read(&f.dir, id)
        {
            names.extend(sc.segments.iter().map(|s| s.file.clone()));
        }
        names.sort();
        names.dedup();
        let side = sidecar::sidecar_file_name(id);
        names.push(sidecar::transcript_file_name(id));
        // Stray tmp files of this sidecar (an interrupted write), whatever their suffix.
        if let Ok(rd) = std::fs::read_dir(&f.dir) {
            let mut tmps: Vec<String> = rd
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| crate::atomic::is_tmp_of(n, &side))
                .collect();
            tmps.sort();
            names.extend(tmps);
        }
        names.push(side);
        let mut removed = Vec::new();
        for n in names {
            match std::fs::remove_file(f.dir.join(&n)) {
                Ok(()) => removed.push(n),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(removed)
    }
}
