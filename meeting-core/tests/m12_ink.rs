//! F17-FR4 / FR5 through the app: timed strokes in world coordinates, erase, lasso, undo and
//! redo, the deferred save and the viewport, and a damaged canvas never saved over.
mod libcommon;

use libcommon::*;
use meeting::app::App;
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

/// A two-point stroke from `(x, y)` written at recording time `t` (negative: untimed).
fn stroke(f: &Fx, layer: &str, x: f32, y: f32, t: f32) -> Value {
    let pts = [
        x,
        y,
        0.5,
        t,
        x + 40.0,
        y,
        0.7,
        if t < 0.0 { -1.0 } else { t + 16.0 },
    ];
    parse(&f.app.ink_stroke_json(&f.id, layer, 0, 3.0, &pts))
}

#[test]
fn timed_strokes_are_kept_in_world_space_and_saved_on_flush() {
    let f = fixture();
    let mut open = parse(&f.app.notes_open_json(&f.id));
    let images = open.as_object_mut().unwrap().remove("imagesDir").unwrap();
    assert!(
        images.as_str().unwrap().ends_with(".notes/images"),
        "{images}"
    );
    assert_eq!(
        open,
        json!({"view": null, "clock": "recording-ms", "bounds": null, "strokes": 0, "items": 0})
    );
    let a = stroke(&f, "canvas", -12_000.0, 48_000.0, 450_000.0);
    assert_eq!(a["strokeId"], 0);
    assert_eq!(a["atMs"], 450_000);
    assert_eq!(a["bbox"], json!([-12_001.5, 47_998.5, -11_958.5, 48_001.5]));
    let b = stroke(&f, "canvas", 100.0, 100.0, -1.0);
    assert_eq!(
        (b["strokeId"].clone(), b["atMs"].clone()),
        (json!(1), Value::Null)
    );
    // The sidecar names the notes from the first stroke; the ink waits for the flush.
    let sc = read_sidecar(&f.dir, &f.id);
    assert_eq!(sc.extra["notes"]["format"], "inkbin-2");
    assert_eq!(sc.extra["notes"]["space"], "world");
    let canvas = notes::path(&f.dir, &f.id, Layer::Canvas);
    assert!(!canvas.exists());
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 0}));
    let saved = notes::load(&f.dir, &f.id, Layer::Canvas).unwrap();
    let s = &saved.strokes()[0];
    assert_eq!((s.rec_ms, s.points[1].timestamp_ms), (Some(450_000), 16));
    assert_eq!(s.points[0].x, -12_000.0);
    assert!(
        s.created_at_ms > 1_700_000_000_000,
        "epoch ms, as inkread's"
    );
    assert_eq!(saved.strokes()[1].rec_ms, None);
    // F17-FR10's ink area is a layer of its own; the sidecar is not named twice.
    let m = stroke(&f, "minutes", 0.0, 0.0, -1.0);
    assert_eq!(m["strokeId"], 0);
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
    assert_eq!(
        notes::load(&f.dir, &f.id, Layer::Minutes)
            .unwrap()
            .strokes()
            .len(),
        1
    );
}

#[test]
fn bad_strokes_are_refused() {
    let f = fixture();
    let err = |r: String| parse(&r)["error"].as_str().unwrap_or("").to_owned();
    let ok = [0.0, 0.0, 1.0, -1.0];
    assert!(err(f.app.ink_stroke_json(&f.id, "page", 0, 3.0, &ok)).contains("layer"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 2, 3.0, &ok)).contains("eraser"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 9, 3.0, &ok)).contains("tool"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", -1, 3.0, &ok)).contains("tool"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 0, 3.0, &[0.0; 3])).contains("quadruples"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 0, 3.0, &[])).contains("quadruples"));
    let nan = [f32::NAN, 0.0, 1.0, 5.0];
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 0, 3.0, &nan)).contains("quadruples"));
    assert!(err(f.app.ink_stroke_json(&f.id, "canvas", 0, 0.0, &ok)).contains("width"));
    assert!(err(f.app.ink_stroke_json("../x", "canvas", 0, 3.0, &ok)).contains("invalid"));
    assert!(
        !err(f
            .app
            .ink_stroke_json("meeting-20990101-000000", "canvas", 0, 3.0, &ok))
        .is_empty()
    );
    // A point with a non-finite time keeps the stroke's.
    let odd = [0.0, 0.0, 1.0, 10.0, 1.0, 1.0, 1.0, f32::INFINITY];
    assert_eq!(
        parse(&f.app.ink_stroke_json(&f.id, "canvas", 1, 30.0, &odd))["atMs"],
        10
    );
    assert!(!read_sidecar(&f.dir, &f.id).extra.is_empty());
}

#[test]
fn erase_lasso_move_delete_and_history() {
    let f = fixture();
    stroke(&f, "canvas", 0.0, 0.0, 1_000.0); // 0
    stroke(&f, "canvas", 0.0, 100.0, 2_000.0); // 1
    stroke(&f, "canvas", 5_000.0, 5_000.0, 3_000.0); // 2
    let gone = parse(
        &f.app
            .ink_erase_json(&f.id, "canvas", &[20.0, -30.0, 20.0, 130.0], 6.0),
    );
    assert_eq!(gone["erased"], json!([0, 1]));
    assert_eq!(gone["bbox"][3], 101.5);
    let miss = parse(
        &f.app
            .ink_erase_json(&f.id, "canvas", &[-900.0, -900.0], 6.0),
    );
    assert_eq!(miss, json!({"erased": [], "refs": [], "bbox": null}));
    let u = parse(&f.app.ink_history_json(&f.id, "canvas", false));
    assert_eq!(
        (u["changed"].clone(), u["canRedo"].clone()),
        (json!(true), json!(true))
    );
    let r = parse(&f.app.ink_history_json(&f.id, "canvas", true));
    assert_eq!(r["changed"], true);
    f.app.ink_history_json(&f.id, "canvas", false);
    // A loop round stroke 2, far out.
    let lasso = [
        4_900.0, 4_900.0, 5_100.0, 4_900.0, 5_100.0, 5_100.0, 4_900.0, 5_100.0,
    ];
    let sel = parse(&f.app.ink_lasso_json(&f.id, "canvas", &lasso));
    assert_eq!(sel["ids"], json!([2]));
    let mv = parse(&f.app.ink_move_json(&f.id, "canvas", "[2]", -5_000.0, 0.0));
    assert_eq!(mv["moved"], true);
    assert_eq!(mv["bbox"], json!([-1.5, 4_998.5, 5_041.5, 5_001.5]));
    let none = parse(&f.app.ink_move_json(&f.id, "canvas", "[42]", 1.0, 1.0));
    assert_eq!(none, json!({"moved": false, "bbox": null}));
    let view = parse(
        &f.app
            .ink_strokes_json(&f.id, "canvas", [-10.0, 4_900.0, 100.0, 5_100.0]),
    );
    assert_eq!(view["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(view["strokes"][0]["id"], 2);
    assert_eq!(view["strokes"][0]["atMs"], 3_000);
    assert_eq!(
        view["strokes"][0]["pts"],
        json!([0.0, 5_000.0, 40.0, 5_000.0])
    );
    assert_eq!(view["strokes"][0]["color"], json!([0, 0, 0, 255]));
    assert_eq!(view["canUndo"], true);
    let del = parse(&f.app.ink_delete_json(&f.id, "canvas", "[2, 7]"));
    assert_eq!(del["deleted"], json!([2]));
    // Undo everything: the delete, the move, then the three strokes (the erase was undone).
    for _ in 0..5 {
        assert_eq!(
            parse(&f.app.ink_history_json(&f.id, "canvas", false))["changed"],
            true
        );
    }
    let end = parse(&f.app.ink_history_json(&f.id, "canvas", false));
    assert_eq!(
        end,
        json!({"changed": false, "bbox": null, "canUndo": false, "canRedo": true})
    );
    for bad in [
        f.app.ink_erase_json(&f.id, "canvas", &[1.0], 6.0),
        f.app.ink_erase_json(&f.id, "page", &[1.0, 1.0], 6.0),
        f.app.ink_lasso_json(&f.id, "canvas", &[f32::NAN, 1.0]),
        f.app.ink_move_json(&f.id, "canvas", "two", 1.0, 1.0),
        f.app.ink_delete_json(&f.id, "page", "[1]"),
        f.app.ink_history_json(&f.id, "page", true),
        f.app.ink_strokes_json(&f.id, "page", [0.0; 4]),
        f.app.ink_strokes_json("../x", "canvas", [0.0; 4]),
        f.app.ink_erase_json("../x", "canvas", &[1.0, 1.0], 6.0),
        f.app.ink_lasso_json("../x", "canvas", &[1.0, 1.0]),
        f.app.ink_move_json("../x", "canvas", "[1]", 1.0, 1.0),
        f.app.ink_delete_json("../x", "canvas", "[1]"),
        f.app.ink_history_json("../x", "canvas", false),
    ] {
        assert!(parse(&bad).get("error").is_some(), "{bad}");
    }
}

#[test]
fn closing_saves_the_ink_and_the_viewport() {
    let f = fixture();
    stroke(&f, "canvas", 10.0, 10.0, 5.0);
    assert_eq!(
        parse(&f.app.notes_close_json(&f.id, 250.0, -40.0, 0.5)),
        json!({"saved": 1})
    );
    let open = parse(&f.app.notes_open_json(&f.id));
    assert_eq!(open["view"], json!({"cx": 250.0, "cy": -40.0, "zoom": 0.5}));
    assert_eq!(open["strokes"], 1);
    // Closing a page that never opened its canvas keeps no view and saves nothing.
    assert_eq!(
        parse(
            &f.app
                .notes_close_json("meeting-20261007-211707", 0.0, 0.0, 1.0)
        ),
        json!({"saved": 0})
    );
    assert!(parse(&f.app.notes_open_json("../x")).get("error").is_some());
}

#[test]
fn a_damaged_canvas_is_never_saved_over_and_save_errors_are_reported() {
    let f = fixture();
    let canvas = notes::path(&f.dir, &f.id, Layer::Canvas);
    std::fs::create_dir_all(canvas.parent().unwrap()).unwrap();
    std::fs::write(&canvas, b"INKB\x02\x01junk").unwrap();
    let e = parse(&f.app.notes_open_json(&f.id));
    assert!(e["error"].as_str().unwrap().contains("can't be read"));
    assert!(stroke(&f, "canvas", 0.0, 0.0, 1.0).get("error").is_some());
    assert_eq!(std::fs::read(&canvas).unwrap(), b"INKB\x02\x01junk");
    // The minutes layer opens; its save fails when its file's place is a folder.
    stroke(&f, "minutes", 0.0, 0.0, -1.0);
    std::fs::create_dir_all(notes::path(&f.dir, &f.id, Layer::Minutes)).unwrap();
    assert!(
        parse(&f.app.ink_save_json(&f.id))["error"]
            .as_str()
            .unwrap()
            .contains("can't be saved")
    );
    assert!(
        parse(&f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0))
            .get("error")
            .is_some()
    );
}

#[test]
fn the_first_stroke_reports_a_sidecar_it_cannot_write() {
    let f = fixture();
    let sidecar = f.dir.join(format!("{}.json", f.id));
    std::fs::write(&sidecar, b"{not json").unwrap();
    let r = stroke(&f, "canvas", 0.0, 0.0, 1.0);
    assert!(r.get("error").is_some(), "{r}");
}

#[test]
fn flushing_one_meeting_leaves_anothers_ink() {
    let f = fixture();
    let second = parse(&f.app.finish_json(&f.id, 1_000, ""));
    assert!(second.get("error").is_none());
    let id2 = begin(&f.app, T0 + 60_000)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    stroke(&f, "canvas", 0.0, 0.0, -1.0);
    let pts = [0.0, 0.0, 1.0, 5.0];
    f.app.ink_stroke_json(&id2, "canvas", 0, 3.0, &pts);
    assert_eq!(parse(&f.app.ink_save_json(&id2)), json!({"saved": 1}));
    assert!(!notes::path(&f.dir, &f.id, Layer::Canvas).exists());
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
}

/// F17-FR7 / F17-AC4 (host part): Listen's tap gives the stroke's time, where playback starts
/// (2 s before, in its segment) and the transcript line spoken then; an untimed stroke and an
/// empty spot give no seek.
#[test]
fn listen_finds_the_moment_a_stroke_was_written() {
    let f = fixture();
    stroke(&f, "canvas", 0.0, 0.0, 450_000.0);
    stroke(&f, "canvas", 0.0, 500.0, -1.0);
    let hit = parse(&f.app.ink_hit_json(&f.id, "canvas", 20.0, 1.0, 6.0));
    assert_eq!(hit["strokeId"], 0);
    assert_eq!(hit["atMs"], 450_000);
    assert_eq!(hit["seekMs"], 448_000);
    assert_eq!(
        (hit["segIndex"].clone(), hit["segOffsetMs"].clone()),
        (json!(0), json!(448_000))
    );
    assert_eq!(hit["lineN"], Value::Null, "no transcript lines yet");
    assert_eq!(
        parse(&f.app.ink_hit_json(&f.id, "canvas", 20.0, 500.0, 6.0)),
        json!({"strokeId": 1, "atMs": null})
    );
    assert_eq!(
        parse(&f.app.ink_hit_json(&f.id, "canvas", 900.0, 900.0, 6.0)),
        json!({"strokeId": null})
    );
    assert!(
        parse(&f.app.ink_hit_json(&f.id, "page", 0.0, 0.0, 6.0))
            .get("error")
            .is_some()
    );
    assert!(
        parse(&f.app.ink_hit_json("../x", "canvas", 0.0, 0.0, 6.0))
            .get("error")
            .is_some()
    );
    // The sidecar unreadable: no seek can be placed.
    std::fs::write(f.dir.join(format!("{}.json", f.id)), b"{bad").unwrap();
    assert!(
        parse(&f.app.ink_hit_json(&f.id, "canvas", 20.0, 1.0, 6.0))
            .get("error")
            .is_some()
    );
}

/// F17-FR12 (F13-FR6): the notes page never touches the recording. Writing, saving and
/// closing the page leave the meeting recording, its segments and its liveness as they were;
/// only the sidecar's `notes` object is added.
#[test]
fn the_page_is_independent_of_the_recording() {
    let f = fixture();
    let before = read_sidecar(&f.dir, &f.id);
    stroke(&f, "canvas", 0.0, 0.0, 1_000.0);
    f.app.ink_save_json(&f.id);
    f.app.notes_close_json(&f.id, 0.0, 0.0, 1.0);
    let after = read_sidecar(&f.dir, &f.id);
    assert_eq!(after.status, meeting::status::Status::Recording);
    assert_eq!(after.segments, before.segments);
    assert_eq!(after.duration_ms, before.duration_ms);
    let mut added = after.extra.clone();
    assert!(added.remove("notes").is_some());
    assert_eq!(added, before.extra);
    assert_eq!(f.app.liveness().recording.as_deref(), Some(f.id.as_str()));
    // And the recording finishing with the page open loses no ink.
    stroke(&f, "canvas", 0.0, 100.0, 2_000.0);
    f.app.finish_json(&f.id, 5_000, "");
    assert_eq!(parse(&f.app.ink_save_json(&f.id)), json!({"saved": 1}));
    assert_eq!(
        notes::load(&f.dir, &f.id, Layer::Canvas)
            .unwrap()
            .strokes()
            .len(),
        2
    );
}
