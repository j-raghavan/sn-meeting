//! The notes canvas's ink (F17, ADR D17): strokes, erase, lasso, undo and the `.inkbin` codec.
//!
//! Adapted from inkread's `inkread-ink` crate (`~/Workspace/inkread/inkread-ink`, AGPL-3.0-only,
//! same owner; see `LICENSES-3RDPARTY.md`). What changed for an infinite canvas (R20, OQ28):
//! - **World coordinates.** inkread clamps every point to a `[0,1]` page (`model.rs:131-148`)
//!   and keeps moves on the page (`:237`). Here a layer is unbounded `f32` world space, where
//!   one unit is one screen pixel at 100 % zoom; points are only checked to be finite.
//! - **Recording-clock times** (OQ14 proposed option (b)). `created_at_ms` keeps inkread's
//!   meaning (epoch ms, caller-supplied). A stroke also carries `rec_ms`, its first point's
//!   time on the recording clock (F15-FR3), or `None` when written outside a recording.
//! - **`.inkbin` v2** (`codec.rs`): the same little-endian, hardened layout plus a space byte
//!   and the optional `rec_ms`. inkread's page layers and v1 stay as they are in inkread.
//! - **One gesture, one undo step.** An eraser sweep or a lasso delete is one edit.
//!
//! Pure and host-testable: no clock, no I/O (the notes files are `crate::notes`).

pub mod codec;
pub mod model;
pub mod select;

pub use codec::{INKBIN_VERSION, decode_layer, encode_layer};
pub use model::{BBox, InkColor, InkError, InkLayer, InkPoint, Stroke, StrokeId, Tool};
pub use select::{select_in_polygon, selection_bounds};
