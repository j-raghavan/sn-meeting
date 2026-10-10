//! F21 (M16) through the notes files and the app: the scene (`canvas.inkbin` v2 plus
//! `items.json`), its one history, and what the canvas draws.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::canvas::Ref;
use meeting::notes::{self, Layer};
use serde_json::{Value, json};
use std::path::PathBuf;

struct Fx {
    _e: Env,
    app: App,
    id: String,
    dir: PathBuf,
}

fn fixture() -> Fx {
    let e = Env::new();
    let app = e.app();
    let id = begin(&app, T0)["id"].as_str().unwrap().to_owned();
    Fx {
        dir: e.export(),
        _e: e,
        app,
        id,
    }
}

fn stroke(f: &Fx, x: f32, y: f32, t: f32) -> Value {
    let pts = [x, y, 0.5, t, x + 40.0, y, 0.7, t + 16.0];
    parse(&f.app.ink_stroke_json(&f.id, "canvas", 0, 3.0, &pts))
}

fn items_path(f: &Fx) -> PathBuf {
    notes::folder(&f.dir, &f.id).join(notes::ITEMS)
}

/// One item of each type, a bound arrow, a link, an unknown field and an unknown type.
fn every_item() -> Value {
    let style =
        json!({"color": "blue", "opacity": 1.0, "fill": "semi", "dash": "draw", "size": "m"});
    json!([
        {"id": "rect1", "type": "rectangle", "x": 0.0, "y": 0.0, "width": 100.0, "height": 50.0,
         "rotation": 0.0, "style": style, "recMs": 1000, "createdAt": "2026-10-09T10:00:00-07:00",
         "groupId": "g-1", "futureField": {"a": 1}},
        {"id": "ell1", "type": "ellipse", "x": 300.0, "y": 0.0, "width": 80.0, "height": 80.0,
         "rotation": 0.5, "style": style, "recMs": null},
        {"id": "line1", "type": "line", "x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0,
         "rotation": 0.0, "start": {"x": 0.0, "y": 200.0, "bind": null},
         "end": {"x": 90.0, "y": 210.0, "bind": null}, "style": style, "recMs": null},
        {"id": "arr1", "type": "arrow", "x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0,
         "rotation": 0.0, "start": {"x": 50.0, "y": 25.0, "bind": "i:rect1"},
         "end": {"x": 340.0, "y": 40.0, "bind": "i:ell1"}, "style": style, "recMs": null,
         "link": {"kind": "meeting", "target": "meeting-20261008-090000"}},
        {"id": "text1", "type": "text", "x": 0.0, "y": 400.0, "width": 320.0, "height": 40.0,
         "rotation": 0.0, "style": style, "text": "Budget owner?", "recMs": 761000},
        {"id": "note1", "type": "note", "x": 400.0, "y": 400.0, "width": 200.0, "height": 200.0,
         "rotation": 0.0, "style": style, "text": "Ask Sam", "recMs": null},
        {"id": "tab1", "type": "table", "x": 0.0, "y": 700.0, "width": 320.0, "height": 96.0,
         "rotation": 0.0, "style": style, "recMs": null,
         "table": {"rows": 2, "cols": 2, "cells": ["a", "b", "c", "d"],
                   "rowMinHeights": [48.0, 48.0], "cellHeights": [0.0, 0.0, 0.0, 0.0],
                   "cellRecMs": [null, null, null, null]}},
        {"id": "img1", "type": "image", "x": 600.0, "y": 0.0, "width": 160.0, "height": 120.0,
         "rotation": 0.0, "style": style, "recMs": null,
         "image": {"file": "img1.jpg", "width": 1600, "height": 1200}},
        {"id": "odd1", "type": "sparkle", "x": 900.0, "y": 0.0, "width": 10.0, "height": 10.0,
         "rotation": 0.0, "style": style, "recMs": null, "glitter": true}
    ])
}

/// F21-AC1: 3 strokes and one item of each type, plus an unknown field and an unknown type,
/// round-trip; `canvas.inkbin` is byte-identical to the file written before the items came,
/// and the v2 decoder still reads it.
#[test]
fn every_item_round_trips_beside_an_untouched_inkbin() {
    let f = fixture();
    for i in 0..3 {
        #[allow(clippy::cast_precision_loss)]
        stroke(&f, 0.0, 1000.0 + 50.0 * i as f32, 1000.0 * i as f32);
    }
    assert_eq!(
        parse(&f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0)),
        json!({"saved": 1})
    );
    let inkbin = notes::path(&f.dir, &f.id, Layer::Canvas);
    let before = std::fs::read(&inkbin).unwrap();
    assert!(!items_path(&f).exists(), "strokes alone need no items.json");
    let file = json!({"version": 1, "order": [], "items": every_item(), "strokes": {},
                      "envelopeExtra": "kept"});
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();

    let (mut scene, report) = notes::load_scene(&f.dir, &f.id, 1).unwrap();
    assert_eq!(report.dropped + report.stale, 0);
    assert_eq!(scene.doc().items.len(), 9);
    assert!(
        scene
            .move_refs(&[Ref::Item("note1".into())], 10.0, 0.0)
            .is_some()
    );
    assert!(scene.undo().is_some());
    assert!(notes::save_scene(&f.dir, &f.id, 1, &mut scene).unwrap());
    assert_eq!(
        std::fs::read(&inkbin).unwrap(),
        before,
        "canvas.inkbin untouched"
    );
    assert!(meeting::ink::decode_layer(&before).is_ok());

    let saved: Value = serde_json::from_slice(&std::fs::read(items_path(&f)).unwrap()).unwrap();
    assert_eq!(
        saved["items"],
        every_item(),
        "every item, unknown field and type included"
    );
    assert_eq!(saved["envelopeExtra"], "kept");
    assert_eq!(
        saved["order"],
        json!([
            "s:0", "s:1", "s:2", "i:rect1", "i:ell1", "i:line1", "i:arr1", "i:text1", "i:note1",
            "i:tab1", "i:img1", "i:odd1"
        ])
    );
    let (again, _) = notes::load_scene(&f.dir, &f.id, 1).unwrap();
    assert_eq!(again.doc(), scene.doc());
    assert!(
        !notes::save_scene(&f.dir, &f.id, 1, &mut scene).unwrap(),
        "nothing to write"
    );

    // The app draws them in paint order, ends resolved, the unknown type as unknown.
    let all = parse(
        &f.app
            .canvas_for_draw_json(&f.id, [-5000.0, -5000.0, 5000.0, 5000.0]),
    );
    let entries = all["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 12);
    let arrow = &entries[6]["item"];
    assert_eq!(arrow["bound"], json!([true, true]));
    assert_eq!(arrow["ends"][0], 100.0, "on rect1's right edge");
    assert_eq!(entries[11]["item"]["known"], false);
    assert_eq!(all["itemsOk"], true);
    let open = parse(&f.app.notes_open_json(&f.id));
    assert_eq!(
        (open["strokes"].clone(), open["items"].clone()),
        (json!(3), json!(9))
    );
}

/// F21-AC2: `items.json` names stroke 7, which a build without F21 erased: its entries are
/// dropped on load, nothing else changes, and the next save names it nowhere.
#[test]
fn references_to_an_erased_stroke_are_dropped() {
    let f = fixture();
    for i in 0..3 {
        #[allow(clippy::cast_precision_loss)]
        stroke(&f, 100.0 * i as f32, 0.0, -1.0);
    }
    f.app.ink_save_json(&f.id);
    let file = json!({"version": 1, "order": ["s:2", "s:7", "s:0", "i:rect1", "s:1"],
        "items": [every_item()[0]],
        "strokes": {"7": {"groupId": "g-1", "link": null}, "1": {"groupId": "g-1", "link": null}}});
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    let (mut scene, report) = notes::load_scene(&f.dir, &f.id, 1).unwrap();
    assert_eq!(report.stale, 2);
    let order: Vec<String> = scene.order().iter().map(ToString::to_string).collect();
    assert_eq!(order, ["s:2", "s:0", "i:rect1", "s:1"]);
    assert_eq!(scene.doc().extras.len(), 1);
    assert_eq!(
        scene.item("rect1").unwrap().group_id.as_deref(),
        Some("g-1")
    );
    assert!(notes::save_scene(&f.dir, &f.id, 1, &mut scene).unwrap());
    let text = std::fs::read_to_string(items_path(&f)).unwrap();
    assert!(!text.contains("s:7") && !text.contains("\"7\""), "{text}");
}

/// F21-FR1: an unreadable `items.json` refuses item edits and is never written; strokes keep
/// working.
#[test]
fn an_unreadable_items_file_is_kept_and_strokes_still_work() {
    let f = fixture();
    stroke(&f, 0.0, 0.0, -1.0);
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    std::fs::write(items_path(&f), b"{\"version\": 1, oops").unwrap();
    assert_eq!(stroke(&f, 0.0, 100.0, -1.0)["strokeId"], 1);
    let drawn = parse(
        &f.app
            .canvas_for_draw_json(&f.id, [-500.0, -500.0, 500.0, 500.0]),
    );
    assert_eq!(drawn["itemsOk"], false);
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
    assert_eq!(
        std::fs::read(items_path(&f)).unwrap(),
        b"{\"version\": 1, oops"
    );
    assert_eq!(
        notes::load(&f.dir, &f.id, Layer::Canvas)
            .unwrap()
            .strokes()
            .len(),
        2
    );
    // A folder where items.json should be is unreadable too.
    let g = fixture();
    std::fs::create_dir_all(items_path(&g)).unwrap();
    let (s, _) = notes::load_scene(&g.dir, &g.id, 1).unwrap();
    assert!(!s.items_ok());
    assert!(
        parse(&g.app.canvas_for_draw_json("../x", [0.0; 4]))
            .get("error")
            .is_some()
    );
}

/// F21-FR1: undo and redo are scene-wide; the stroke calls answer in stroke ids still.
#[test]
fn one_history_for_strokes_and_items() {
    let f = fixture();
    let file = json!({"version": 1, "items": [every_item()[0]]});
    std::fs::create_dir_all(notes::folder(&f.dir, &f.id)).unwrap();
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    stroke(&f, 0.0, 300.0, 5.0);
    let lasso = [-10.0, 290.0, 60.0, 290.0, 60.0, 310.0, -10.0, 310.0];
    let sel = parse(&f.app.ink_lasso_json(&f.id, "canvas", &lasso));
    assert_eq!(
        (sel["ids"].clone(), sel["refs"].clone()),
        (json!([0]), json!(["s:0"]))
    );
    let drawn = parse(
        &f.app
            .canvas_for_draw_json(&f.id, [-500.0, -500.0, 500.0, 500.0]),
    );
    assert_eq!(drawn["selection"], json!(["s:0"]));
    assert_eq!(
        drawn["entries"][0]["ref"], "i:rect1",
        "the file's item under the new stroke"
    );
    assert_eq!(drawn["entries"][1]["ref"], "s:0");
    let u = parse(&f.app.ink_history_json(&f.id, "canvas", false));
    assert_eq!(u["changed"], true);
    let drawn = parse(
        &f.app
            .canvas_for_draw_json(&f.id, [-500.0, -500.0, 500.0, 500.0]),
    );
    assert_eq!(
        drawn["selection"],
        json!([]),
        "an undone stroke leaves the selection"
    );
    assert_eq!(drawn["canRedo"], true);
    // Minutes ink is a layer of its own with its own history.
    let pts = [0.0, 0.0, 1.0, -1.0];
    f.app.ink_stroke_json(&f.id, "minutes", 0, 3.0, &pts);
    let m = parse(
        &f.app
            .ink_strokes_json(&f.id, "minutes", [-10.0, -10.0, 10.0, 10.0]),
    );
    assert_eq!(m["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 2}));
}

fn edit(f: &Fx, op: Value) -> Value {
    parse(&f.app.canvas_edit_json(&f.id, &op.to_string()))
}

/// F21-AC4 / OQ34: Clear canvas on 40 timed strokes and 5 items (an image, a group, a bound
/// arrow, a link) is one step that rewrites both files; Undo restores everything identically
/// and the image stays. After a restart the hidden backup restores it.
#[test]
fn clear_canvas_is_one_step_and_survives_a_restart() {
    let f = fixture();
    for i in 0..40 {
        #[allow(clippy::cast_precision_loss)]
        stroke(&f, 0.0, 2000.0 + 20.0 * i as f32, 1000.0 * i as f32);
    }
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    let all = every_item();
    let five = json!([all[0], all[1], all[3], all[7], all[4]]);
    let file =
        json!({"version": 1, "items": five, "strokes": {"3": {"groupId": "g-1", "link": null}}});
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    let images = notes::folder(&f.dir, &f.id).join(notes::IMAGES);
    std::fs::create_dir_all(&images).unwrap();
    std::fs::write(images.join("img1.jpg"), b"\xff\xd8\xff jpeg").unwrap();
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    let before_ink = std::fs::read(notes::path(&f.dir, &f.id, Layer::Canvas)).unwrap();
    let draw = |f: &Fx| parse(&f.app.canvas_for_draw_json(&f.id, [-1e6, -1e6, 1e6, 1e6]));
    assert_eq!(draw(&f)["canRestore"], false);
    let before = draw(&f)["entries"].clone();
    assert_eq!(before.as_array().unwrap().len(), 45);

    let r = edit(&f, json!({"op": "clear"}));
    assert!(r["dirty"].is_array() && r["canUndo"] == true, "{r}");
    assert_eq!(draw(&f)["entries"], json!([]));
    assert_eq!(draw(&f)["canRestore"], true);
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
    let cleared: Value = serde_json::from_slice(&std::fs::read(items_path(&f)).unwrap()).unwrap();
    assert_eq!(cleared["items"], json!([]));
    assert!(
        notes::load(&f.dir, &f.id, Layer::Canvas)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        edit(&f, json!({"op": "clear"}))["dirty"],
        Value::Null,
        "nothing left to clear"
    );

    let u = parse(&f.app.ink_history_json(&f.id, "canvas", false));
    assert_eq!(u["changed"], true);
    f.app.ink_save_json(&f.id);
    assert_eq!(
        std::fs::read(notes::path(&f.dir, &f.id, Layer::Canvas)).unwrap(),
        before_ink
    );
    assert_eq!(
        draw(&f)["entries"],
        before,
        "ids, times, order, groups, bindings and the link, identically"
    );
    assert!(images.join("img1.jpg").exists());

    // Cleared again, then the app restarts: Undo is gone, the backup is not.
    edit(&f, json!({"op": "clear"}));
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    assert!(
        !images.join("img1.jpg").exists(),
        "no canvas refers to it once closed"
    );
    let h = parse(&f.app.ink_history_json(&f.id, "canvas", false));
    assert_eq!(h["changed"], false);
    let r = edit(&f, json!({"op": "restoreCleared"}));
    assert!(r["dirty"].is_array(), "{r}");
    let d = draw(&f);
    assert_eq!(d["entries"].as_array().unwrap().len(), 45);
    assert!(
        images.join("img1.jpg").exists(),
        "the image came back from the backup"
    );
    assert!(
        edit(&f, json!({"op": "zap"}))["error"]
            .as_str()
            .unwrap()
            .contains("unknown")
    );
    assert!(
        parse(&f.app.canvas_edit_json(&f.id, "nope"))
            .get("error")
            .is_some()
    );
    std::fs::remove_dir_all(notes::folder(&f.dir, &f.id).join(notes::CLEARED)).unwrap();
    assert!(
        edit(&f, json!({"op": "restoreCleared"}))["error"]
            .as_str()
            .unwrap()
            .contains("no cleared")
    );
}

#[test]
fn edits_are_refused_while_items_json_is_unreadable() {
    let f = fixture();
    stroke(&f, 0.0, 0.0, -1.0);
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    std::fs::write(items_path(&f), b"junk").unwrap();
    assert!(
        edit(&f, json!({"op": "clear"}))["error"]
            .as_str()
            .unwrap()
            .contains("can't be read")
    );
    assert!(
        parse(&f.app.canvas_edit_json("../x", "{}"))
            .get("error")
            .is_some()
    );
    // A backup that can't be written leaves the canvas as it was.
    let g = fixture();
    stroke(&g, 0.0, 0.0, -1.0);
    let notes_dir = notes::folder(&g.dir, &g.id);
    std::fs::create_dir_all(&notes_dir).unwrap();
    std::fs::write(notes_dir.join(notes::CLEARED), b"a file in its place").unwrap();
    assert!(
        edit(&g, json!({"op": "clear"}))["error"]
            .as_str()
            .unwrap()
            .contains("can't be cleared")
    );
    assert_eq!(
        parse(&g.app.canvas_for_draw_json(&g.id, [-9.0, -9.0, 99.0, 99.0]))["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// F21-FR4 through the app: select by rectangle, move all as one step, duplicate, delete.
#[test]
fn select_move_duplicate_and_delete() {
    let f = fixture();
    let all = every_item();
    let file = json!({"version": 1, "items": [all[0], all[1]]});
    std::fs::create_dir_all(notes::folder(&f.dir, &f.id)).unwrap();
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    stroke(&f, 0.0, 60.0, 5.0);
    let r = edit(
        &f,
        json!({"op": "select", "rect": [390.0, 70.0, -10.0, 0.0]}),
    );
    assert_eq!(r["selection"], json!(["i:rect1", "i:ell1", "s:0"]), "{r}");
    assert!(r["selectionBounds"].is_array());
    let m = edit(&f, json!({"op": "move", "dx": 0.0, "dy": 100.0}));
    assert!(m["dirty"].is_array());
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e5, -1e5, 1e5, 1e5]));
    assert_eq!(d["entries"][0]["item"]["y"], 100.0);
    assert_eq!(
        parse(&f.app.ink_history_json(&f.id, "canvas", false))["changed"],
        true
    );
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e5, -1e5, 1e5, 1e5]));
    assert_eq!(d["entries"][0]["item"]["y"], 0.0, "one step for all three");
    let t = edit(&f, json!({"op": "select", "at": [50.0, 25.0], "r": 6.0}));
    assert_eq!(t["selection"], json!(["i:rect1"]));
    let dup = edit(&f, json!({"op": "duplicate"}));
    assert_eq!(dup["selection"].as_array().unwrap().len(), 1);
    assert_ne!(dup["selection"][0], "i:rect1");
    let del = edit(&f, json!({"op": "delete", "refs": ["i:ell1"]}));
    assert!(del["dirty"].is_array());
    assert_eq!(
        edit(&f, json!({"op": "select", "none": true}))["selection"],
        json!([])
    );
    let lasso = edit(
        &f,
        json!({"op": "select", "polygon": [-10.0, -10.0, 120.0, -10.0, 120.0, 70.0, -10.0, 70.0]}),
    );
    assert_eq!(lasso["selection"], json!(["i:rect1", "s:0"]));
    assert_eq!(
        edit(&f, json!({"op": "select", "refs": ["s:0"]}))["selection"],
        json!(["s:0"])
    );
    assert!(edit(&f, json!({"op": "select"}))["error"].is_string());
    assert!(edit(&f, json!({"op": "move", "dx": "x"}))["error"].is_string());
}

/// F21-FR5 through the app: handles, arranging and grouping, each one step.
#[test]
fn handles_arrange_and_group() {
    let f = fixture();
    let all = every_item();
    let file = json!({"version": 1, "items": [all[0], all[1], all[3], all[4]]});
    std::fs::create_dir_all(notes::folder(&f.dir, &f.id)).unwrap();
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    let ok = |v: Value| assert!(v["dirty"].is_array(), "{v}");
    ok(edit(
        &f,
        json!({"op": "resize", "ref": "i:rect1", "corner": "br", "x": 200.0, "y": 100.0}),
    ));
    ok(edit(
        &f,
        json!({"op": "resize", "ref": "i:text1", "corner": "br", "x": 200.0, "y": 410.0, "fit": 90.0}),
    ));
    ok(edit(
        &f,
        json!({"op": "rotate", "ref": "i:ell1", "x": 500.0, "y": 40.0}),
    ));
    ok(edit(
        &f,
        json!({"op": "endpoint", "ref": "i:arr1", "which": "end", "x": 900.0, "y": 900.0}),
    ));
    ok(edit(
        &f,
        json!({"op": "group", "refs": ["i:rect1", "i:ell1"]}),
    ));
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e5, -1e5, 1e5, 1e5]));
    let items: Vec<&Value> = d["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["item"])
        .collect();
    assert_eq!(
        (items[0]["width"].clone(), items[3]["height"].clone()),
        (json!(200.0), json!(90.0))
    );
    assert_eq!(
        items[1]["rotation"].as_f64().unwrap(),
        std::f64::consts::FRAC_PI_2
    );
    assert_eq!(items[2]["bound"], json!([true, false]));
    assert_eq!(items[0]["groupId"], items[1]["groupId"]);
    assert_eq!(d["selection"], json!(["i:rect1", "i:ell1"]));
    ok(edit(&f, json!({"op": "back", "refs": ["i:text1"]})));
    ok(edit(&f, json!({"op": "front"})));
    ok(edit(&f, json!({"op": "ungroup"})));
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e5, -1e5, 1e5, 1e5]));
    let refs: Vec<&str> = d["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["ref"].as_str().unwrap())
        .collect();
    assert_eq!(refs, ["i:text1", "i:arr1", "i:rect1", "i:ell1"]);
    assert!(
        edit(
            &f,
            json!({"op": "resize", "ref": "i:rect1", "corner": "zz", "x": 1, "y": 1})
        )["error"]
            .is_string()
    );
    assert!(edit(&f, json!({"op": "rotate", "ref": "nope", "x": 1, "y": 1}))["error"].is_string());
    assert_eq!(
        edit(&f, json!({"op": "group", "refs": ["i:rect1"]}))["dirty"],
        Value::Null
    );
}

/// F21-AC9 (host part): a shape and a stroke restyled blue, then L, one step each; the
/// default becomes {color: blue, size: l, …} and survives a restart.
#[test]
fn a_style_pick_restyles_and_becomes_the_default() {
    let f = fixture();
    let file = json!({"version": 1, "items": [every_item()[0]]});
    std::fs::create_dir_all(notes::folder(&f.dir, &f.id)).unwrap();
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    stroke(&f, 0.0, 300.0, 5.0);
    edit(&f, json!({"op": "select", "refs": ["i:rect1", "s:0"]}));
    let r = edit(
        &f,
        json!({"op": "restyle", "property": "color", "value": "blue"}),
    );
    assert!(r["dirty"].is_array(), "{r}");
    assert!(
        edit(
            &f,
            json!({"op": "restyle", "property": "size", "value": "l"})
        )["dirty"]
            .is_array()
    );
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e5, -1e5, 1e5, 1e5]));
    assert_eq!(d["entries"][1]["stroke"]["style"]["color"], "blue");
    assert_eq!(d["entries"][1]["stroke"]["width"], 5.0);
    assert_eq!(d["defaultStyle"]["color"], "blue");
    assert_eq!(d["defaultStyle"]["size"], "l");
    let again = f._e.app();
    let s = parse(&again.get_settings_json());
    assert_eq!(s["settings"]["notes"]["defaultStyle"]["size"], "l", "{s}");
    assert!(
        edit(
            &f,
            json!({"op": "setDefaultStyle", "property": "opacity", "value": 0.5})
        )["dirty"]
            .is_null()
    );
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [0.0; 4]));
    assert_eq!(d["defaultStyle"]["opacity"], 0.5);
    assert!(edit(&f, json!({"op": "restyle", "property": "color"}))["error"].is_string());
    assert!(edit(&f, json!({"op": "restyle", "value": "red"}))["error"].is_string());
}

/// F21-FR7: the Pen draws with the default style: its true colour in `.inkbin`, the size's
/// width, the style ids beside it, each point's pressure drawn; still one timed stroke.
#[test]
fn styled_pen_strokes_keep_colour_size_and_time() {
    let f = fixture();
    let pts = [0.0, 0.0, 0.2, 761_000.0, 40.0, 0.0, 0.9, 761_016.0];
    let style = r#"{"color":"red","opacity":0.5,"size":"xl"}"#;
    let r = parse(&f.app.ink_stroke_styled_json(&f.id, 0, style, &pts));
    assert_eq!(
        (r["strokeId"].clone(), r["atMs"].clone()),
        (json!(0), json!(761_000))
    );
    let h = parse(&f.app.ink_stroke_styled_json(&f.id, 1, style, &pts));
    assert_eq!(h["strokeId"], 1);
    let d = parse(
        &f.app
            .canvas_for_draw_json(&f.id, [-100.0, -100.0, 100.0, 100.0]),
    );
    let pen = &d["entries"][0]["stroke"];
    assert_eq!(pen["color"], json!([0xE0, 0x31, 0x31, 128]));
    assert_eq!(pen["width"], 10.0);
    assert_eq!(pen["style"]["color"], "red");
    assert_eq!(pen["pressure"].as_array().unwrap().len(), 2);
    let hl = &d["entries"][1]["stroke"];
    assert_eq!(
        hl["color"],
        json!([0, 0, 0, 64]),
        "the highlighter is unchanged"
    );
    assert!(hl.get("pressure").is_none());
    assert!(
        parse(&f.app.ink_stroke_styled_json(&f.id, 0, "nope", &pts))
            .get("error")
            .is_some()
    );
}

/// F21-FR8 / AC9: a rectangle drawn after a pick takes the default style; an arrow drawn from
/// it binds; a bad shape is refused.
#[test]
fn new_shapes_take_the_default_style_and_arrows_attach() {
    let f = fixture();
    edit(
        &f,
        json!({"op": "setDefaultStyle", "property": "color", "value": "blue"}),
    );
    edit(
        &f,
        json!({"op": "setDefaultStyle", "property": "size", "value": "l"}),
    );
    let r = edit(
        &f,
        json!({"op": "create", "type": "rectangle", "from": [0, 0], "to": [100, 50], "recMs": 1200}),
    );
    let rect = r["created"].as_str().unwrap().to_owned();
    assert!(r["dirty"].is_array());
    let a = edit(
        &f,
        json!({"op": "create", "type": "arrow", "from": [50, 25], "to": [400, 25]}),
    );
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e4, -1e4, 1e4, 1e4]));
    let item = &d["entries"][0]["item"];
    assert_eq!(
        (
            item["style"]["color"].clone(),
            item["style"]["size"].clone()
        ),
        (json!("blue"), json!("l"))
    );
    assert_eq!(item["recMs"], 1200);
    assert_eq!(d["entries"][1]["ref"], a["created"]);
    assert_eq!(d["entries"][1]["item"]["bound"], json!([true, false]));
    assert_eq!(
        d["entries"][1]["item"]["ends"][0], 100.0,
        "on the rectangle's edge"
    );
    assert!(rect.starts_with("i:"));
    assert!(
        edit(
            &f,
            json!({"op": "create", "type": "star", "from": [0, 0], "to": [1, 1]})
        )["error"]
            .is_string()
    );
    assert!(
        edit(
            &f,
            json!({"op": "create", "type": "line", "from": [0], "to": [1, 1]})
        )["error"]
            .is_string()
    );
}

/// F21-FR9: a text box placed with its first text is one step, timed then; editing it again
/// is one more; a blank one is discarded.
#[test]
fn text_boxes_through_the_app() {
    let f = fixture();
    let r = edit(
        &f,
        json!({"op": "create", "type": "text", "at": [10, 20], "text": "Budget owner?",
                            "height": 40.0, "recMs": 761_000}),
    );
    let t = r["created"].as_str().unwrap().to_owned();
    let n = edit(
        &f,
        json!({"op": "create", "type": "note", "at": [500, 500], "text": "Ask Sam", "height": 30.0}),
    );
    assert!(n["created"].is_string());
    assert!(
        edit(
            &f,
            json!({"op": "create", "type": "text", "at": [0, 0], "text": ""})
        )["error"]
            .is_string()
    );
    let e = edit(
        &f,
        json!({"op": "setText", "ref": t, "text": "Budget owner: Sam", "height": 70.0, "recMs": 5}),
    );
    assert!(e["dirty"].is_array());
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e4, -1e4, 1e4, 1e4]));
    let it = &d["entries"][0]["item"];
    assert_eq!(
        (
            it["text"].clone(),
            it["height"].clone(),
            it["recMs"].clone()
        ),
        (json!("Budget owner: Sam"), json!(70.0), json!(761_000))
    );
    assert_eq!(d["entries"][1]["item"]["height"], 200.0);
    assert!(edit(&f, json!({"op": "setText", "ref": t}))["error"].is_string());
}

/// F21-FR10 through the app: a table from a drag, a cell typed into, rows and columns where
/// the target is; a hit names the cell and its time.
#[test]
fn tables_through_the_app() {
    let f = fixture();
    let r = edit(
        &f,
        json!({"op": "create", "type": "table", "from": [0, 0], "to": [480, 144], "recMs": 1000}),
    );
    let t = r["created"].as_str().unwrap().to_owned();
    let c = edit(
        &f,
        json!({"op": "table.setCell", "ref": t, "row": 1, "col": 2, "text": "Sam",
                            "height": 30.0, "recMs": 761_000}),
    );
    assert!(c["dirty"].is_array());
    let h = parse(&f.app.canvas_hit_json(&f.id, 400.0, 70.0, 6.0));
    assert_eq!(
        (h["ref"].clone(), h["cell"].clone()),
        (json!(t), json!({"row": 1, "col": 2}))
    );
    assert_eq!(
        (h["atMs"].clone(), h["seekMs"].clone()),
        (json!(761_000), json!(759_000))
    );
    let h0 = parse(&f.app.canvas_hit_json(&f.id, 10.0, 10.0, 6.0));
    assert_eq!(h0["atMs"], 1000, "an untyped cell: the table's own time");
    assert_eq!(
        parse(&f.app.canvas_hit_json(&f.id, -500.0, 0.0, 6.0))["ref"],
        Value::Null
    );
    for op in ["table.addRow", "table.removeRow"] {
        assert!(
            edit(&f, json!({"op": op, "ref": t, "row": 0}))["dirty"].is_array(),
            "{op}"
        );
    }
    for op in ["table.addColumn", "table.removeColumn"] {
        assert!(
            edit(&f, json!({"op": op, "ref": t, "col": 0}))["dirty"].is_array(),
            "{op}"
        );
    }
    assert!(
        edit(
            &f,
            json!({"op": "table.resizeRow", "ref": t, "row": 0, "height": 99.0})
        )["dirty"]
            .is_array()
    );
    assert!(edit(&f, json!({"op": "table.addRow", "ref": t}))["error"].is_string());
    assert!(
        edit(
            &f,
            json!({"op": "table.setCell", "ref": t, "row": 0, "col": 0})
        )["error"]
            .is_string()
    );
    assert!(
        parse(&f.app.canvas_hit_json("../x", 0.0, 0.0, 1.0))
            .get("error")
            .is_some()
    );
    std::fs::write(f.dir.join(format!("{}.json", f.id)), b"{bad").unwrap();
    assert!(
        parse(&f.app.canvas_hit_json(&f.id, 400.0, 70.0, 6.0))
            .get("error")
            .is_some()
    );
}

const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 0xFF, 0xD9,
];

fn images_dir(f: &Fx) -> PathBuf {
    notes::folder(&f.dir, &f.id).join(notes::IMAGES)
}

fn image_files(f: &Fx) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(images_dir(f))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// F21-AC11: a real JPEG named photo.JPEG is copied byte for byte to images/<itemId>.jpg with
/// its pixel size; text named .png, a 25 MB file, a 60 MP image, a symlink and a `../` name are
/// each refused with a message and nothing is written. Unreferenced copies go at close.
#[test]
fn images_are_checked_copied_and_cleaned_up() {
    let f = fixture();
    let src = tempfile::tempdir().unwrap();
    let photo = src.path().join("photo.JPEG");
    std::fs::write(&photo, JPEG).unwrap();
    let view = r#"{"rect": [0, 0, 1000, 1000], "recMs": 5000}"#;
    let r =
        parse(
            &f.app
                .canvas_import_image_json(&f.id, photo.to_str().unwrap(), 1600, 1200, view),
        );
    let item = r["itemId"].as_str().unwrap().to_owned();
    assert!(r["dirty"].is_array());
    assert_eq!(
        std::fs::read(images_dir(&f).join(format!("{item}.jpg"))).unwrap(),
        JPEG
    );
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e4, -1e4, 1e4, 1e4]));
    let it = &d["entries"][0]["item"];
    assert_eq!(
        it["image"],
        json!({"file": format!("{item}.jpg"), "width": 1600, "height": 1200})
    );
    assert_eq!(
        (it["width"].clone(), it["recMs"].clone()),
        (json!(600.0), json!(5000))
    );

    let before = image_files(&f);
    let text = src.path().join("text.png");
    std::fs::write(&text, b"just some words").unwrap();
    let big = src.path().join("big.png");
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.resize(25 * 1024 * 1024, 0);
    std::fs::write(&big, b).unwrap();
    let link = src.path().join("link.jpg");
    std::os::unix::fs::symlink(&photo, &link).unwrap();
    let dotdot = format!(
        "{}/../{}/photo.JPEG",
        src.path().display(),
        src.path().file_name().unwrap().to_string_lossy()
    );
    for (p, w, h, says) in [
        (text.to_str().unwrap().to_owned(), 10, 10, "isn't the image"),
        (big.to_str().unwrap().to_owned(), 10, 10, "20 MB"),
        (
            photo.to_str().unwrap().to_owned(),
            10_000,
            6_000,
            "50 megapixels",
        ),
        (link.to_str().unwrap().to_owned(), 10, 10, "plain image"),
        (dotdot, 10, 10, "isn't allowed"),
        ("/nope/x.gif".to_owned(), 10, 10, "PNG, JPG and WebP"),
        ("/nope/x.png".to_owned(), 10, 10, "can't be read"),
        (photo.to_str().unwrap().to_owned(), 0, 10, "can't be read"),
    ] {
        let e = parse(&f.app.canvas_import_image_json(&f.id, &p, w, h, view));
        assert!(e["error"].as_str().unwrap_or("").contains(says), "{p}: {e}");
    }
    assert_eq!(image_files(&f), before, "nothing written");
    assert!(
        parse(
            &f.app
                .canvas_import_image_json(&f.id, photo.to_str().unwrap(), 1, 1, "x")
        )["error"]
            .is_string()
    );
    assert!(
        parse(
            &f.app
                .canvas_import_image_json(&f.id, photo.to_str().unwrap(), 1, 1, "{}")
        )["error"]
            .is_string()
    );
    assert!(
        parse(
            &f.app
                .canvas_import_image_json(&f.id, photo.to_str().unwrap(), -1, 1, view)
        )["error"]
            .is_string()
    );

    // A second copy whose item is undone is deleted when the page closes; the first stays.
    let r2 = parse(
        &f.app
            .canvas_import_image_json(&f.id, photo.to_str().unwrap(), 16, 12, view),
    );
    assert!(r2["itemId"].is_string());
    assert_eq!(image_files(&f).len(), 2);
    f.app.ink_history_json(&f.id, "canvas", false);
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    assert_eq!(image_files(&f), vec![format!("{item}.jpg")]);
}

/// F21-AC12 (storage): New canvas makes canvas-2/ and canvases.json; canvas 1 keeps its paths
/// and bytes; strokes on canvas 2 carry rec_ms; the meeting reopens on the canvas shown last.
#[test]
fn new_canvases_live_beside_the_first() {
    let f = fixture();
    stroke(&f, 0.0, 0.0, 1_000.0);
    edit(
        &f,
        json!({"op": "create", "type": "rectangle", "from": [0, 0], "to": [10, 10]}),
    );
    f.app.ink_save_json(&f.id);
    let one = notes::path(&f.dir, &f.id, Layer::Canvas);
    let one_bytes = std::fs::read(&one).unwrap();
    let r = parse(&f.app.canvas_new_json(&f.id));
    assert_eq!(r, json!({"n": 2}));
    assert!(notes::folder(&f.dir, &f.id).join("canvas-2").is_dir());
    assert!(notes::folder(&f.dir, &f.id).join(notes::CANVASES).is_file());
    assert_eq!(stroke(&f, 500.0, 500.0, 761_000.0)["atMs"], 761_000);
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e4, -1e4, 1e4, 1e4]));
    assert_eq!(
        d["entries"].as_array().unwrap().len(),
        1,
        "canvas 2 holds only its own"
    );
    f.app.ink_save_json(&f.id);
    assert_eq!(
        std::fs::read(&one).unwrap(),
        one_bytes,
        "canvas 1 keeps its paths and bytes"
    );
    let two = notes::load_scene(&f.dir, &f.id, 2).unwrap().0;
    assert_eq!(two.ink().strokes()[0].rec_ms, Some(761_000));
    let list = parse(&f.app.canvas_list_json(&f.id));
    assert_eq!(list["current"], 2);
    assert_eq!(list["canvases"][0]["strokes"], 1);
    assert_eq!(list["canvases"][0]["items"], 1);
    assert_eq!(list["canvases"][1]["strokes"], 1);
    // Reopened, the meeting shows canvas 2; showing 1 goes back, and 1 is kept for next time.
    f.app.notes_close_json(&f.id, 5.0, 6.0, 2.0);
    let open = parse(&f.app.notes_open_json(&f.id));
    assert_eq!(
        (open["canvas"].clone(), open["view"]["zoom"].clone()),
        (json!(2), json!(2.0))
    );
    let s1 = parse(&f.app.canvas_show_json(&f.id, 1));
    assert_eq!(
        (s1["n"].clone(), s1["strokes"].clone(), s1["items"].clone()),
        (json!(1), json!(1), json!(1))
    );
    assert!(s1["defaultStyle"].is_object());
    assert!(parse(&f.app.canvas_show_json(&f.id, 9))["error"].is_string());
    assert!(parse(&f.app.canvas_new_json("../x"))["error"].is_string());
    assert!(parse(&f.app.canvas_list_json("../x"))["error"].is_string());
    assert!(parse(&f.app.canvas_show_json("../x", 1))["error"].is_string());
    // An empty canvas can be deleted from Canvases…; one with ink, canvas 1 or the shown one can't.
    assert_eq!(parse(&f.app.canvas_new_json(&f.id)), json!({"n": 3}));
    parse(&f.app.canvas_show_json(&f.id, 1));
    assert_eq!(
        edit(&f, json!({"op": "deleteCanvas", "n": 3})),
        json!({"deleted": 3})
    );
    assert!(!notes::folder(&f.dir, &f.id).join("canvas-3").exists());
    for n in [1, 2, 7] {
        assert!(
            edit(&f, json!({"op": "deleteCanvas", "n": n}))["error"].is_string(),
            "{n}"
        );
    }
    assert!(edit(&f, json!({"op": "deleteCanvas"}))["error"].is_string());
    // Clear on canvas 2 keeps its own backup.
    parse(&f.app.canvas_show_json(&f.id, 2));
    edit(&f, json!({"op": "clear"}));
    assert_eq!(notes::cleared_canvas(&f.dir, &f.id), Some(2));
    assert_eq!(
        parse(&f.app.canvas_for_draw_json(&f.id, [0.0; 4]))["canRestore"],
        true
    );
    parse(&f.app.canvas_show_json(&f.id, 1));
    assert_eq!(
        parse(&f.app.canvas_for_draw_json(&f.id, [0.0; 4]))["canRestore"],
        false
    );
}

/// F21-AC13: a link to a meeting is stored and drawn with its badge; `../x` is refused; once
/// that meeting is deleted, a badge tap says it is missing.
#[test]
fn links_to_other_meetings() {
    let f = fixture();
    f.app.finish_json(&f.id, 1_000, "");
    let other = begin(&f.app, T0 + 60_000)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    f.app.finish_json(&other, 1_000, "");
    let r = edit(
        &f,
        json!({"op": "create", "type": "rectangle", "from": [0, 0], "to": [100, 50]}),
    );
    let rect = r["created"].as_str().unwrap().to_owned();
    edit(&f, json!({"op": "select", "refs": [rect]}));
    assert!(
        edit(&f, json!({"op": "link", "target": "../x"}))["error"]
            .as_str()
            .unwrap()
            .contains("isn't a meeting")
    );
    assert!(edit(&f, json!({"op": "link", "target": other}))["dirty"].is_array());
    let d = parse(&f.app.canvas_for_draw_json(&f.id, [-1e4, -1e4, 1e4, 1e4]));
    assert_eq!(
        d["entries"][0]["item"]["link"],
        json!({"kind": "meeting", "target": other, "followed": true})
    );
    let t = parse(&f.app.canvas_link_target_json(&f.id, &rect));
    assert_eq!(
        t,
        json!({"meetingId": other, "exists": true, "missing": false})
    );
    assert!(parse(&f.app.delete_json(&other)).get("error").is_none());
    let gone = parse(&f.app.canvas_link_target_json(&f.id, &rect));
    assert_eq!(gone["missing"], true, "{gone}");
    assert_eq!(
        parse(&f.app.canvas_link_target_json(&f.id, "s:9")),
        json!({"meetingId": null})
    );
    assert!(parse(&f.app.canvas_link_target_json(&f.id, "bad"))["error"].is_string());
    assert!(parse(&f.app.canvas_link_target_json("../x", "s:1"))["error"].is_string());
    assert!(edit(&f, json!({"op": "unlink"}))["dirty"].is_array());
    let file = json!({"version": 1, "items": [{"id": "n1", "type": "rectangle", "link": {"kind": "note", "target": "/a.note"}}]});
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    std::fs::write(items_path(&f), serde_json::to_vec(&file).unwrap()).unwrap();
    assert_eq!(
        parse(&f.app.canvas_link_target_json(&f.id, "i:n1")),
        json!({"kind": "note", "followed": false})
    );
}
