//! F21 (ADR D21): the notes tools as sn-canvas. A canvas is a **scene**: the ink layer
//! (`canvas.inkbin` v2, unchanged; `crate::ink`) plus an item list in a sibling `items.json`,
//! with one undo history for both. The model, its geometry and its edits are a Rust port of
//! sn-canvas's pure Kotlin model (`~/Workspace/sn-canvas/android/app/src/main/java/com/
//! sncanvas/canvas/`, MIT, Copyright (c) 2026 JRaghavan; notice in `LICENSES-3RDPARTY.md`),
//! keeping its rules and constants; its JUnit cases are carried as test vectors. The shell
//! measures text, decodes image headers and draws what [`draw`] hands it.
pub mod draw;
pub mod edits;
pub mod geom;
pub mod hit;
pub mod item;
pub mod scene;
pub mod style;

pub use item::{Item, Ref};
pub use scene::{Doc, LoadReport, Scene};
pub use style::Style;
