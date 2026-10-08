//! The meeting edits the UI makes: to-dos (F6-FR2).
use super::list::Source;
use super::{LibError, Library, invalid};
use crate::sidecar;
use serde::Serialize;

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
}
