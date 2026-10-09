//! F18: Calendar Home's core side. Events come only from read-only iCalendar links the user
//! lists in `MyStyle/Meetings/calendar-links.txt` (D18); Meetings never writes to a calendar
//! or to that file (INV16), and a link URL never leaves the core (INV15).
pub mod cache;
pub mod fetch;
pub mod ics;
pub mod links;
