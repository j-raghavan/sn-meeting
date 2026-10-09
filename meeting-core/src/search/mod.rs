//! F19: Search. A rebuildable full-text cache of the meeting files (ADR D19, INV17).
pub mod index;
pub mod query;
pub mod units;

pub use index::{Group, GroupKind, Index};
pub use units::{EventDoc, Kind, Unit};
