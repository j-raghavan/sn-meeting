//! F18: Calendar Home's core side. Events come only from read-only iCalendar links the user
//! lists in `MyStyle/Meetings/calendar-links.txt` (D18); Meetings never writes to a calendar
//! or to that file (INV16), and a link URL never leaves the core (INV15).
pub mod cache;
pub mod fetch;
pub mod ics;
pub mod links;

/// The process's calendar state, owned by `App`.
#[derive(Debug, Default)]
pub struct Calendar {
    /// The links file, re-read when it changes (F18-FR1).
    pub links: links::LinksCache,
    /// Held by a refresh: one at a time.
    pub busy: std::sync::Mutex<()>,
    /// How fetches are bounded; tests allow the mock server's `http:` (F18-FR3).
    pub cfg: fetch::FetchCfg,
}
