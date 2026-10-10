//! F21-FR1 / R21: what the shell draws, in paint order, with the geometry already resolved
//! (bound arrow ends, table row heights), so the painters never recompute the model's rules.
use super::item::{Item, Ref, StrokeExtra};
use super::scene::Scene;
use crate::ink::{BBox, Stroke, StrokeId};
use serde_json::{Value, json};

/// `[x0, y0, x1, y1]`, or `null`.
#[must_use]
pub fn bbox_json(b: Option<BBox>) -> Value {
    b.map_or(Value::Null, |b| json!([b.x0, b.y0, b.x1, b.y1]))
}

/// A stroke as the view draws it: `{id, tool, width, color, atMs, pts: [x, y, …]}` (F17), plus
/// its pressures and, from `items.json`, its style ids, group and link (F21).
#[must_use]
pub fn stroke_json(s: &Stroke, extra: Option<&StrokeExtra>) -> Value {
    let pts: Vec<f32> = s.points.iter().flat_map(|p| [p.x, p.y]).collect();
    let mut v = json!({"id": s.id.0, "tool": s.tool.code(), "width": s.width,
           "color": [s.color.r, s.color.g, s.color.b, s.color.a], "atMs": s.rec_ms, "pts": pts});
    if let Some(e) = extra {
        if let Some(st) = &e.style {
            v["style"] = st.to_value();
        }
        if let Some(g) = &e.group_id {
            v["groupId"] = json!(g);
        }
        if let Some(l) = &e.link {
            v["link"] = link_json(l);
        }
    }
    v
}

fn link_json(l: &super::item::Link) -> Value {
    json!({"kind": l.kind, "target": l.target, "followed": l.followed()})
}

/// An item as the view draws it, its ends resolved.
#[must_use]
pub fn item_json(scene: &Scene, it: &Item) -> Value {
    let mut v = json!({"id": it.id, "type": it.kind, "known": it.known(), "x": it.x, "y": it.y,
        "width": it.width, "height": it.height, "rotation": it.rotation,
        "style": it.style.to_value(), "recMs": it.rec_ms});
    if let Some((a, b)) = scene.ends(it) {
        v["ends"] = json!([a.x, a.y, b.x, b.y]);
        let bound = |e: &Option<super::item::End>| e.as_ref().is_some_and(|e| e.bind.is_some());
        v["bound"] = json!([bound(&it.start), bound(&it.end)]);
    }
    if let Some(t) = &it.text {
        v["text"] = json!(t);
    }
    if let Some(t) = &it.table {
        v["table"] = json!({"rows": t.rows, "cols": t.cols, "cells": t.cells,
                            "rowHeights": t.row_heights()});
    }
    if let Some(i) = &it.image {
        v["image"] = json!({"file": i.file, "width": i.width, "height": i.height});
    }
    if let Some(g) = &it.group_id {
        v["groupId"] = json!(g);
    }
    if let Some(l) = &it.link {
        v["link"] = link_json(l);
    }
    v
}

/// One paint-ordered entry: `{ref, stroke}` or `{ref, item}`.
#[must_use]
pub fn entry_json(scene: &Scene, r: &Ref) -> Option<Value> {
    match r {
        Ref::Stroke(n) => scene.ink().get(StrokeId(*n)).map(
            |s| json!({"ref": r.to_string(), "stroke": stroke_json(s, scene.doc().extras.get(n))}),
        ),
        Ref::Item(i) => scene
            .item(i)
            .map(|it| json!({"ref": r.to_string(), "item": item_json(scene, it)})),
    }
}

/// `nativeCanvasForDraw`: everything meeting the world rectangle, in paint order, with the
/// whole canvas's bounds, the history state and the selection.
#[must_use]
pub fn for_draw(scene: &Scene, rect: BBox) -> Value {
    let entries: Vec<Value> = scene
        .order()
        .iter()
        .filter(|r| scene.bounds_of_ref(r).is_some_and(|b| b.meets(rect)))
        .filter_map(|r| entry_json(scene, r))
        .collect();
    json!({"entries": entries, "bounds": bbox_json(scene.bounds()),
           "canUndo": scene.can_undo(), "canRedo": scene.can_redo(),
           "selection": refs_json(&scene.selection),
           "selectionBounds": bbox_json(scene.bounds_of(&scene.selection)),
           "itemsOk": scene.items_ok()})
}

/// Refs as strings.
#[must_use]
pub fn refs_json(refs: &[Ref]) -> Value {
    Value::Array(refs.iter().map(|r| json!(r.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::item::{End, Image, Link, Table};
    use crate::canvas::scene::tests::{pen, rect, with_item};
    use crate::canvas::style::Style;
    use crate::ink::InkLayer;

    #[test]
    fn entries_come_in_paint_order_with_resolved_geometry() {
        let mut s = Scene::load(InkLayer::new(), Ok(None)).0;
        let st = pen(&mut s, &[(0.0, 0.0), (10.0, 0.0)], Some(3));
        with_item(&mut s, rect("a", 100.0, 0.0));
        let mut ar = Item::new("ar", "arrow", Style::default());
        ar.start = Some(End {
            x: 0.0,
            y: 0.0,
            bind: Some(Ref::Item("a".into())),
        });
        ar.end = Some(End {
            x: 500.0,
            y: 25.0,
            bind: None,
        });
        ar.link = Link::meeting("meeting-20261008-090000");
        ar.group_id = Some("g".into());
        with_item(&mut s, ar);
        let mut t = rect("t", 0.0, 300.0);
        t.kind = "table".into();
        t.table = Some(Table::empty(2, 2));
        t.text = Some("x".into());
        t.image = Some(Image {
            file: "t.png".into(),
            width: 2,
            height: 2,
        });
        with_item(&mut s, t);
        s.selection = vec![Ref::Stroke(st.0)];
        let all = BBox {
            x0: -1000.0,
            y0: -1000.0,
            x1: 1000.0,
            y1: 1000.0,
        };
        let v = for_draw(&s, all);
        let refs: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["ref"].as_str().unwrap())
            .collect();
        assert_eq!(refs, ["s:0", "i:a", "i:ar", "i:t"]);
        assert_eq!(v["entries"][0]["stroke"]["atMs"], 3);
        assert_eq!(
            v["entries"][2]["item"]["ends"],
            json!([200.0, 25.0, 500.0, 25.0])
        );
        assert_eq!(v["entries"][2]["item"]["bound"], json!([true, false]));
        assert_eq!(v["entries"][2]["item"]["link"]["followed"], true);
        assert_eq!(v["entries"][2]["item"]["groupId"], "g");
        assert_eq!(
            v["entries"][3]["item"]["table"]["rowHeights"],
            json!([48.0, 48.0])
        );
        assert_eq!(v["entries"][3]["item"]["image"]["file"], "t.png");
        assert_eq!(v["selection"], json!(["s:0"]));
        assert_eq!(v["itemsOk"], true);
        let none = BBox {
            x0: 5000.0,
            y0: 5000.0,
            x1: 6000.0,
            y1: 6000.0,
        };
        assert_eq!(for_draw(&s, none)["entries"], json!([]));
        assert_eq!(entry_json(&s, &Ref::Stroke(99)), None);
        assert_eq!(bbox_json(None), Value::Null);
        let e = StrokeExtra {
            style: Some(Style::default()),
            group_id: Some("g".into()),
            link: Link::meeting("meeting-20261008-090000"),
            ..StrokeExtra::default()
        };
        let sj = stroke_json(s.ink().get(st).unwrap(), Some(&e));
        assert_eq!(
            (sj["style"]["size"].clone(), sj["groupId"].clone()),
            (json!("m"), json!("g"))
        );
        assert_eq!(sj["link"]["kind"], "meeting");
    }
}
