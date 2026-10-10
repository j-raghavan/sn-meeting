// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasModel.kt`, MIT,
// Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
//! F21-FR1: the items beside the strokes. sn-canvas's `Element` without freehand points (the
//! strokes stay in `canvas.inkbin`): a bbox item (`x/y/width/height`, `rotation` about its
//! centre) or a line/arrow with two ends, each free or bound to another item or stroke. Every
//! item also has a style, an optional group and link, its creation time and `recMs`. Stored
//! items are untrusted (Security): [`Item::from_value`] applies F21's caps, and fields this
//! build doesn't know are kept in `extra` and written back.
use super::style::Style;
use crate::ink::StrokeId;
use serde_json::{Map, Value, json};
use std::fmt;

/// The item types this build edits (F21-FR1). Any other type is kept and drawn as a dashed
/// placeholder box that can be moved or deleted but not edited.
pub const TYPES: [&str; 8] = [
    "rectangle",
    "ellipse",
    "line",
    "arrow",
    "text",
    "note",
    "table",
    "image",
];

/// Text in one item (or one table cell) is at most this many bytes (Data Model caps).
pub const MAX_TEXT: usize = 64 * 1024;
/// A table's limits (`TableElements.kt:14-20`).
pub const MAX_ROWS: usize = 12;
pub const MAX_COLS: usize = 8;
pub const CELL_WIDTH: f64 = 160.0;
pub const MIN_ROW_HEIGHT: f64 = 48.0;
pub const MAX_ROW_HEIGHT: f64 = 4000.0;
/// A coordinate further out than this is not a canvas (it keeps every sum finite).
pub const MAX_COORD: f64 = 1.0e9;

/// A reference to a stroke (`s:<id>`) or an item (`i:<id>`) of one canvas.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ref {
    Stroke(u32),
    Item(String),
}

impl Ref {
    /// `s:12` or `i:a3f90c1e`; anything else is `None`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Ref> {
        if let Some(n) = s.strip_prefix("s:") {
            return n.parse().ok().map(Ref::Stroke);
        }
        s.strip_prefix("i:")
            .filter(|i| is_plain_id(i))
            .map(|i| Ref::Item(i.to_owned()))
    }

    /// The stroke this names, if it names one.
    #[must_use]
    pub fn stroke(&self) -> Option<StrokeId> {
        match self {
            Ref::Stroke(n) => Some(StrokeId(*n)),
            Ref::Item(_) => None,
        }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Stroke(n) => write!(f, "s:{n}"),
            Ref::Item(i) => write!(f, "i:{i}"),
        }
    }
}

/// An item, group or image id: `[A-Za-z0-9_-]{1,64}`.
#[must_use]
pub fn is_plain_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A copied image's file name: `[A-Za-z0-9_-]+\.[a-z]+` (`CanvasModel.kt:189-205`), so no
/// stored name reaches outside `images/`.
#[must_use]
pub fn is_image_name(s: &str) -> bool {
    let Some((stem, ext)) = s.rsplit_once('.') else {
        return false;
    };
    is_plain_id(stem) && !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_lowercase())
}

/// One end of a line or arrow: where it was put, and the shape it is bound to (resolved live,
/// never stored as a position; `CanvasCore.kt:86-104`).
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    pub x: f64,
    pub y: f64,
    pub bind: Option<Ref>,
}

/// A table's grid (`CanvasModel.kt:106-141`): cells row by row, the height each row was given,
/// and (Meetings) each cell's text height as the shell measured it and when its first text was
/// written (F21-FR15).
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<String>,
    pub row_min_heights: Vec<f64>,
    pub cell_heights: Vec<f64>,
    pub cell_rec_ms: Vec<Option<u64>>,
    pub extra: Map<String, Value>,
}

impl Table {
    /// An empty grid.
    #[must_use]
    pub fn empty(rows: usize, cols: usize) -> Table {
        let rows = rows.clamp(1, MAX_ROWS);
        let cols = cols.clamp(1, MAX_COLS);
        Table {
            rows,
            cols,
            cells: vec![String::new(); rows * cols],
            row_min_heights: vec![MIN_ROW_HEIGHT; rows],
            cell_heights: vec![0.0; rows * cols],
            cell_rec_ms: vec![None; rows * cols],
            extra: Map::new(),
        }
    }

    /// Where (`row`, `col`) is in `cells`.
    #[must_use]
    pub fn index(&self, row: usize, col: usize) -> Option<usize> {
        (row < self.rows && col < self.cols).then_some(row * self.cols + col)
    }

    /// Each row's height: as tall as it was made, or as its tallest cell's text needs.
    #[must_use]
    pub fn row_heights(&self) -> Vec<f64> {
        (0..self.rows)
            .map(|r| {
                let text = (0..self.cols)
                    .map(|c| self.cell_heights[r * self.cols + c])
                    .fold(0.0, f64::max);
                self.row_min_heights[r].max(text)
            })
            .collect()
    }

    fn from_value(v: &Value) -> Option<Table> {
        let o = v.as_object()?;
        let rows = usize::try_from(o.get("rows")?.as_u64()?).ok()?;
        let cols = usize::try_from(o.get("cols")?.as_u64()?).ok()?;
        if !(1..=MAX_ROWS).contains(&rows) || !(1..=MAX_COLS).contains(&cols) {
            return None;
        }
        let n = rows * cols;
        let cells: Vec<String> = o
            .get("cells")?
            .as_array()?
            .iter()
            .map(|c| c.as_str().map(str::to_owned))
            .collect::<Option<_>>()?;
        if cells.len() != n || cells.iter().any(|c| c.len() > MAX_TEXT) {
            return None;
        }
        let heights = |key: &str, len: usize, lo: f64| -> Option<Vec<f64>> {
            match o.get(key) {
                None => Some(vec![lo; len]),
                Some(a) => {
                    let v: Vec<f64> = a
                        .as_array()?
                        .iter()
                        .map(Value::as_f64)
                        .collect::<Option<_>>()?;
                    (v.len() == len)
                        .then(|| v.iter().map(|h| h.clamp(lo, MAX_ROW_HEIGHT)).collect())
                }
            }
        };
        let row_min_heights = heights("rowMinHeights", rows, MIN_ROW_HEIGHT)?;
        let cell_heights = heights("cellHeights", n, 0.0)?;
        let cell_rec_ms = match o.get("cellRecMs") {
            Some(Value::Array(a)) if a.len() == n => a.iter().map(Value::as_u64).collect(),
            _ => vec![None; n],
        };
        let mut extra = o.clone();
        for k in [
            "rows",
            "cols",
            "cells",
            "rowMinHeights",
            "cellHeights",
            "cellRecMs",
        ] {
            extra.remove(k);
        }
        Some(Table {
            rows,
            cols,
            cells,
            row_min_heights,
            cell_heights,
            cell_rec_ms,
            extra,
        })
    }

    fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        o.insert("rows".into(), json!(self.rows));
        o.insert("cols".into(), json!(self.cols));
        o.insert("cells".into(), json!(self.cells));
        o.insert("rowMinHeights".into(), json!(self.row_min_heights));
        o.insert("cellHeights".into(), json!(self.cell_heights));
        o.insert("cellRecMs".into(), json!(self.cell_rec_ms));
        Value::Object(o)
    }
}

/// An image's copy in `images/` and its size in pixels (`CanvasModel.kt:189-205`).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

/// Where an item links to (F21-FR13): `{kind: "meeting", target: <meeting id>}`. A link of
/// a kind this build doesn't know is kept but not followed (`CanvasModel.kt:154-182`).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub kind: String,
    pub target: String,
    pub extra: Map<String, Value>,
}

/// The one link kind Meetings follows.
pub const LINK_MEETING: &str = "meeting";

impl Link {
    /// A link to another meeting; `None` unless `target` is a meeting id.
    #[must_use]
    pub fn meeting(target: &str) -> Option<Link> {
        crate::sidecar::is_valid_id(target).then(|| Link {
            kind: LINK_MEETING.into(),
            target: target.into(),
            extra: Map::new(),
        })
    }

    /// Whether this build follows it.
    #[must_use]
    pub fn followed(&self) -> bool {
        self.kind == LINK_MEETING
    }

    /// A stored link: a meeting link to a non-meeting id is refused; another kind is kept.
    #[must_use]
    pub fn from_value(v: &Value) -> Option<Link> {
        let o = v.as_object()?;
        let kind = o.get("kind")?.as_str()?.to_owned();
        let target = o.get("target")?.as_str()?.to_owned();
        if target.is_empty() || target.len() > 1024 {
            return None;
        }
        if kind == LINK_MEETING && !crate::sidecar::is_valid_id(&target) {
            return None;
        }
        let mut extra = o.clone();
        extra.remove("kind");
        extra.remove("target");
        Some(Link {
            kind,
            target,
            extra,
        })
    }

    /// The stored form.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        o.insert("kind".into(), json!(self.kind));
        o.insert("target".into(), json!(self.target));
        Value::Object(o)
    }
}

/// One item (F21-FR1, Data Model `items.json` v1).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// One of [`TYPES`], or a type this build doesn't know (a placeholder).
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Radians about the box's centre, clockwise on screen.
    pub rotation: f64,
    /// Line and arrow only.
    pub start: Option<End>,
    pub end: Option<End>,
    pub style: Style,
    pub text: Option<String>,
    pub table: Option<Table>,
    pub image: Option<Image>,
    pub group_id: Option<String>,
    pub link: Option<Link>,
    /// The recording-clock time it was made (INV13), or `None`: untimed.
    pub rec_ms: Option<u64>,
    /// Local wall time, for display only.
    pub created_at: Option<String>,
    pub extra: Map<String, Value>,
}

/// The keys [`Item`] reads; everything else is `extra`.
const ITEM_KEYS: [&str; 17] = [
    "id",
    "type",
    "x",
    "y",
    "width",
    "height",
    "rotation",
    "start",
    "end",
    "style",
    "text",
    "table",
    "image",
    "groupId",
    "link",
    "recMs",
    "createdAt",
];

fn coord(o: &Map<String, Value>, k: &str) -> Option<f64> {
    match o.get(k) {
        None | Some(Value::Null) => Some(0.0),
        Some(v) => v.as_f64().filter(|x| x.is_finite() && x.abs() <= MAX_COORD),
    }
}

fn end_of(v: Option<&Value>) -> Option<Option<End>> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Some(None);
    };
    let o = v.as_object()?;
    let x = coord(o, "x")?;
    let y = coord(o, "y")?;
    let bind = o.get("bind").and_then(Value::as_str).and_then(Ref::parse);
    Some(Some(End { x, y, bind }))
}

fn end_value(e: &End) -> Value {
    json!({"x": e.x, "y": e.y, "bind": e.bind.as_ref().map(ToString::to_string)})
}

impl Item {
    /// A new item of `kind` with the default `style`, nothing else set.
    #[must_use]
    pub fn new(id: &str, kind: &str, style: Style) -> Item {
        Item {
            id: id.into(),
            kind: kind.into(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            rotation: 0.0,
            start: None,
            end: None,
            style,
            text: None,
            table: None,
            image: None,
            group_id: None,
            link: None,
            rec_ms: None,
            created_at: None,
            extra: Map::new(),
        }
    }

    /// Whether this build edits this type.
    #[must_use]
    pub fn known(&self) -> bool {
        TYPES.contains(&self.kind.as_str())
    }

    /// A line or an arrow (two ends, no box).
    #[must_use]
    pub fn has_ends(&self) -> bool {
        self.start.is_some() && self.end.is_some()
    }

    /// A stored item under F21's caps, or `None` (dropped and counted by the caller): an id
    /// that isn't plain, a number that isn't finite or a size below zero, text over 64 KB, a
    /// table out of its limits, an image name that isn't plain. A bad link or group is left
    /// off rather than dropping the item.
    #[must_use]
    pub fn from_value(v: &Value) -> Option<Item> {
        let o = v.as_object()?;
        let id = o.get("id")?.as_str().filter(|i| is_plain_id(i))?.to_owned();
        let kind = o.get("type")?.as_str()?.to_owned();
        if kind.is_empty() || kind.len() > 64 {
            return None;
        }
        let (x, y, width, height, rotation) = (
            coord(o, "x")?,
            coord(o, "y")?,
            coord(o, "width")?,
            coord(o, "height")?,
            coord(o, "rotation")?,
        );
        if width < 0.0 || height < 0.0 {
            return None;
        }
        let start = end_of(o.get("start"))?;
        let end = end_of(o.get("end"))?;
        if start.is_some() != end.is_some() {
            return None;
        }
        let text = match o.get("text") {
            None | Some(Value::Null) => None,
            Some(t) => Some(t.as_str().filter(|t| t.len() <= MAX_TEXT)?.to_owned()),
        };
        let table = match o.get("table") {
            None | Some(Value::Null) => None,
            Some(t) => Some(Table::from_value(t)?),
        };
        let image = match o.get("image") {
            None | Some(Value::Null) => None,
            Some(i) => {
                let file = i.get("file")?.as_str().filter(|f| is_image_name(f))?;
                let w = u32::try_from(i.get("width")?.as_u64()?)
                    .ok()
                    .filter(|w| *w > 0)?;
                let h = u32::try_from(i.get("height")?.as_u64()?)
                    .ok()
                    .filter(|h| *h > 0)?;
                Some(Image {
                    file: file.into(),
                    width: w,
                    height: h,
                })
            }
        };
        let mut extra = o.clone();
        for k in ITEM_KEYS {
            extra.remove(k);
        }
        Some(Item {
            id,
            kind,
            x,
            y,
            width,
            height,
            rotation,
            start,
            end,
            style: o.get("style").map(Style::from_value).unwrap_or_default(),
            text,
            table,
            image,
            group_id: o
                .get("groupId")
                .and_then(Value::as_str)
                .filter(|g| is_plain_id(g))
                .map(str::to_owned),
            link: o.get("link").and_then(Link::from_value),
            rec_ms: o.get("recMs").and_then(Value::as_u64),
            created_at: o
                .get("createdAt")
                .and_then(Value::as_str)
                .filter(|t| t.len() <= 64)
                .map(str::to_owned),
            extra,
        })
    }

    /// The stored form; optional fields that are unset are left out.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        let mut put = |k: &str, v: Value| {
            o.insert(k.into(), v);
        };
        put("id", json!(self.id));
        put("type", json!(self.kind));
        put("x", json!(self.x));
        put("y", json!(self.y));
        put("width", json!(self.width));
        put("height", json!(self.height));
        put("rotation", json!(self.rotation));
        if let (Some(s), Some(e)) = (&self.start, &self.end) {
            put("start", end_value(s));
            put("end", end_value(e));
        }
        put("style", self.style.to_value());
        if let Some(t) = &self.text {
            put("text", json!(t));
        }
        if let Some(t) = &self.table {
            put("table", t.to_value());
        }
        if let Some(i) = &self.image {
            put(
                "image",
                json!({"file": i.file, "width": i.width, "height": i.height}),
            );
        }
        if let Some(g) = &self.group_id {
            put("groupId", json!(g));
        }
        if let Some(l) = &self.link {
            put("link", l.to_value());
        }
        put("recMs", json!(self.rec_ms));
        if let Some(c) = &self.created_at {
            put("createdAt", json!(c));
        }
        Value::Object(o)
    }
}

/// A stroke's part in the scene (`items.json` → `strokes.<id>`): its style ids, group and link.
/// Its points and times stay in `.inkbin`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrokeExtra {
    pub style: Option<Style>,
    pub group_id: Option<String>,
    pub link: Option<Link>,
    pub extra: Map<String, Value>,
}

impl StrokeExtra {
    /// Whether it says nothing (and need not be stored).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.style.is_none()
            && self.group_id.is_none()
            && self.link.is_none()
            && self.extra.is_empty()
    }

    /// A stored entry; a bad group or link is left off.
    #[must_use]
    pub fn from_value(v: &Value) -> StrokeExtra {
        let Some(o) = v.as_object() else {
            return StrokeExtra::default();
        };
        let mut extra = o.clone();
        for k in ["style", "groupId", "link"] {
            extra.remove(k);
        }
        StrokeExtra {
            style: o
                .get("style")
                .filter(|s| s.is_object())
                .map(Style::from_value),
            group_id: o
                .get("groupId")
                .and_then(Value::as_str)
                .filter(|g| is_plain_id(g))
                .map(str::to_owned),
            link: o.get("link").and_then(Link::from_value),
            extra,
        }
    }

    /// The stored form.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        if let Some(s) = &self.style {
            o.insert("style".into(), s.to_value());
        }
        if let Some(g) = &self.group_id {
            o.insert("groupId".into(), json!(g));
        }
        o.insert(
            "link".into(),
            self.link.as_ref().map_or(Value::Null, Link::to_value),
        );
        Value::Object(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse_and_print() {
        assert_eq!(Ref::parse("s:12"), Some(Ref::Stroke(12)));
        assert_eq!(Ref::parse("i:a3f9"), Some(Ref::Item("a3f9".into())));
        for bad in ["s:-1", "s:x", "i:", "i:../x", "x:1", ""] {
            assert_eq!(Ref::parse(bad), None, "{bad}");
        }
        assert_eq!(Ref::Stroke(3).to_string(), "s:3");
        assert_eq!(Ref::Item("b".into()).to_string(), "i:b");
        assert_eq!(Ref::Stroke(3).stroke(), Some(StrokeId(3)));
        assert_eq!(Ref::Item("b".into()).stroke(), None);
    }

    #[test]
    fn names_are_plain() {
        assert!(is_image_name("a3f90c1e.jpg"));
        for bad in [
            "../x.png", "a.PNG", "a.", ".png", "a/b.png", "noext", "a.p1",
        ] {
            assert!(!is_image_name(bad), "{bad}");
        }
        assert!(!is_plain_id(&"a".repeat(65)));
    }

    #[test]
    fn links_are_meeting_ids_or_kept_unfollowed() {
        assert!(Link::meeting("meeting-20261008-090000").is_some());
        assert!(Link::meeting("../x").is_none());
        let bad = json!({"kind": "meeting", "target": "../x"});
        assert!(Link::from_value(&bad).is_none());
        let other = json!({"kind": "note", "target": "/x.note", "page": 3});
        let l = Link::from_value(&other).unwrap();
        assert!(!l.followed());
        assert_eq!(l.to_value(), other);
        assert!(Link::from_value(&json!({"kind": "note", "target": ""})).is_none());
        assert!(Link::from_value(&json!({"kind": "note"})).is_none());
        assert!(Link::from_value(&json!(3)).is_none());
    }

    fn full() -> Value {
        json!({"id": "a1", "type": "arrow", "x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0,
               "rotation": 0.5,
               "start": {"x": 0.0, "y": 0.0, "bind": "i:b2"}, "end": {"x": 9.0, "y": 9.0, "bind": null},
               "style": {"color": "blue", "opacity": 1.0, "fill": "none", "dash": "draw", "size": "m"},
               "text": "hi", "groupId": "g-1",
               "link": {"kind": "meeting", "target": "meeting-20261008-090000"},
               "recMs": 761000, "createdAt": "2026-10-09T10:12:41-07:00", "future": [1, 2]})
    }

    #[test]
    fn items_round_trip_with_unknown_fields() {
        let it = Item::from_value(&full()).unwrap();
        assert!(it.known() && it.has_ends());
        assert_eq!(it.to_value(), full());
        let mut t = Item::new("t1", "table", Style::default());
        t.table = Some(Table::empty(2, 3));
        t.image = Some(Image {
            file: "t1.png".into(),
            width: 4,
            height: 5,
        });
        let back = Item::from_value(&t.to_value()).unwrap();
        assert_eq!(back, t);
        let odd = Item::from_value(&json!({"id": "u", "type": "sparkle"})).unwrap();
        assert!(!odd.known() && !odd.has_ends());
    }

    #[test]
    fn items_breaking_the_caps_are_refused() {
        let with = |k: &str, v: Value| {
            let mut o = full();
            o[k] = v;
            Item::from_value(&o)
        };
        assert!(with("id", json!("../x")).is_none());
        assert!(with("type", json!("")).is_none());
        assert!(with("x", json!(1e300)).is_none());
        assert!(with("width", json!(-1.0)).is_none());
        assert!(with("text", json!("x".repeat(MAX_TEXT + 1))).is_none());
        assert!(with("text", json!(3)).is_none());
        assert!(with("start", json!(null)).is_none(), "one end alone");
        assert!(with("start", json!({"x": "a"})).is_none());
        assert!(with("table", json!({"rows": 13, "cols": 1, "cells": []})).is_none());
        assert!(
            with(
                "image",
                json!({"file": "../a.png", "width": 1, "height": 1})
            )
            .is_none()
        );
        assert!(with("image", json!({"file": "a.png", "width": 0, "height": 1})).is_none());
        assert!(Item::from_value(&json!([])).is_none());
        // A bad link or group is left off; the item stays.
        let it = with("link", json!({"kind": "meeting", "target": "../x"})).unwrap();
        assert!(it.link.is_none());
        assert!(with("groupId", json!("a b")).unwrap().group_id.is_none());
        assert!(with("x", json!(null)).unwrap().x == 0.0);
    }

    #[test]
    fn tables_are_checked_and_measured() {
        let ok = |v: Value| Table::from_value(&v);
        let base = json!({"rows": 2, "cols": 1, "cells": ["a", "b"], "extraKey": true});
        let t = ok(base.clone()).unwrap();
        assert_eq!(t.row_min_heights, vec![MIN_ROW_HEIGHT; 2]);
        assert_eq!(t.cell_rec_ms, vec![None, None]);
        assert_eq!(t.extra["extraKey"], true);
        assert!(ok(json!({"rows": 2, "cols": 1, "cells": ["a"]})).is_none());
        assert!(ok(json!({"rows": 1, "cols": 9, "cells": vec![""; 9]})).is_none());
        assert!(ok(json!({"rows": 1, "cols": 1, "cells": [1]})).is_none());
        assert!(
            ok(json!({"rows": 1, "cols": 1, "cells": ["a"], "rowMinHeights": [1, 2]})).is_none()
        );
        assert!(
            ok(json!({"rows": 1, "cols": 1, "cells": ["a"], "rowMinHeights": ["x"]})).is_none()
        );
        let clamped = ok(
            json!({"rows": 1, "cols": 2, "cells": ["a", "b"], "rowMinHeights": [1.0],
                                "cellHeights": [90.0, 5000.0], "cellRecMs": [5, null]}),
        )
        .unwrap();
        assert_eq!(clamped.row_min_heights, vec![MIN_ROW_HEIGHT]);
        assert_eq!(clamped.row_heights(), vec![MAX_ROW_HEIGHT]);
        assert_eq!(clamped.cell_rec_ms, vec![Some(5), None]);
        assert_eq!(clamped.index(0, 1), Some(1));
        assert_eq!(clamped.index(1, 0), None);
        assert!(Table::from_value(&json!(1)).is_none());
        assert_eq!(Table::empty(40, 0).rows, MAX_ROWS);
    }

    #[test]
    fn stroke_entries_keep_what_they_say() {
        let v = json!({"style": {"color": "red", "opacity": 1.0, "fill": "none", "dash": "draw",
                                  "size": "l"},
                       "groupId": "g-5b21", "link": null, "later": 1});
        let e = StrokeExtra::from_value(&v);
        assert_eq!(e.to_value(), v);
        assert!(!e.is_empty());
        assert!(StrokeExtra::default().is_empty());
        assert_eq!(StrokeExtra::from_value(&json!(1)), StrokeExtra::default());
    }
}
