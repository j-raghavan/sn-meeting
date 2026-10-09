//! F15-FR5 / FR8: near-live chunks in the library: recorded as they are finalised, their files
//! kept until both their text is stored and their segment closed, and a crashed segment's
//! finished chunks listed so the shell can rebuild it.
mod libcommon;

use libcommon::*;
use meeting::app::App;
use meeting::library::ChunkIn;
use meeting::sidecar::ChunkState;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn chunk_path(e: &Env, index: u32) -> PathBuf {
    e.data
        .path()
        .join("chunks")
        .join(ID0)
        .join(format!("c-{index:04}.m4a"))
}

/// Chunk `index` written to its file and reported.
fn ready(app: &App, e: &Env, index: i32, start: i64, dur: i64) -> Value {
    let p = chunk_path(e, index as u32);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, b"aac").unwrap();
    report(app, &p.to_string_lossy(), index, start, dur)
}

fn report(app: &App, path: &str, index: i32, start: i64, dur: i64) -> Value {
    let c = ChunkIn {
        index,
        path,
        start_offset_ms: start,
        duration_ms: dur,
        speech_ms: 20_000,
    };
    parse(&app.chunk_ready_json(ID0, c))
}

/// Chunk `index` given its text, as the near-live lane does.
fn done(e: &Env, index: usize) {
    let mut sc = read_sidecar(&e.export(), ID0);
    sc.chunks[index].state = ChunkState::Done;
    sc.chunks[index].text = Some("hello".into());
    meeting::sidecar::write(&e.export(), &sc).unwrap();
}

#[test]
fn a_finalised_chunk_is_recorded_pending_with_its_segment_and_overlap() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    assert_eq!(
        ready(&app, &e, 0, 0, 35_000),
        json!({"index": 0, "state": "pending", "added": true})
    );
    // A hard cut: the next chunk starts 1 s before the previous one ended.
    ready(&app, &e, 1, 34_000, 29_000);
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!(sc.chunks.len(), 2);
    let c = &sc.chunks[1];
    assert_eq!(
        (c.seg_index, c.start_offset_ms, c.duration_ms, c.overlap_ms),
        (0, 34_000, 29_000, 1_000)
    );
    assert_eq!(
        (c.state, c.text.as_deref(), c.attempts, c.speech_ms),
        (ChunkState::Pending, None, 0, Some(20_000))
    );
    // In the next segment, after a gapless rollover.
    app.close_segment_json(ID0, 0, 900_000);
    let seg2 = e.export().join(format!("{ID0}-02.m4a"));
    app.add_segment_json(ID0, seg2.to_str().unwrap(), 900_000);
    ready(&app, &e, 2, 900_000, 30_000);
    let sc = read_sidecar(&e.export(), ID0);
    assert_eq!((sc.chunks[2].seg_index, sc.chunks[2].overlap_ms), (1, 0));
    // The same chunk reported again changes nothing.
    assert_eq!(ready(&app, &e, 2, 900_000, 30_000)["added"], false);
    assert_eq!(read_sidecar(&e.export(), ID0).chunks.len(), 3);
    // The written sidecar has the spec's shape.
    let v: Value =
        serde_json::from_slice(&fs::read(e.export().join(format!("{ID0}.json"))).unwrap()).unwrap();
    assert_eq!(
        v["chunks"][1],
        json!({"index": 1, "segIndex": 0, "startOffsetMs": 34_000, "durationMs": 29_000, "overlapMs": 1_000,
               "state": "pending", "text": null, "attempts": 0, "lastError": null, "speechMs": 20_000})
    );
}

#[test]
fn a_chunk_must_be_its_own_file_and_in_order() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    let err = |v: Value| v["error"].as_str().unwrap_or_default().to_owned();
    let elsewhere = e.export().join("c-0000.m4a");
    assert!(
        err(report(&app, &elsewhere.to_string_lossy(), 0, 0, 1000)).starts_with("chunk 0 must be ")
    );
    let p0 = chunk_path(&e, 0).to_string_lossy().into_owned();
    assert_eq!(
        err(report(&app, &p0, -1, 0, 1000)),
        "chunk index is out of range"
    );
    assert_eq!(
        err(report(&app, &p0, 0, -5, 1000)),
        "startOffsetMs must be >= 0"
    );
    assert_eq!(
        err(report(&app, &p0, 0, 0, 0)),
        "durationMs is out of range"
    );
    assert_eq!(
        err(report(&app, &p0, 0, 0, 60_001)),
        "durationMs is out of range"
    );
    let c = ChunkIn {
        index: 0,
        path: &p0,
        start_offset_ms: 0,
        duration_ms: 1,
        speech_ms: -1,
    };
    assert_eq!(
        err(parse(&app.chunk_ready_json("nope", c))),
        "invalid meeting id"
    );
    ready(&app, &e, 3, 0, 30_000);
    assert_eq!(
        err(ready(&app, &e, 2, 30_000, 30_000)),
        "chunk 2 is out of order"
    );
    // An unknown speech count (-1, a re-cut) is stored as absent.
    let p4 = chunk_path(&e, 4);
    fs::write(&p4, b"x").unwrap();
    let c = ChunkIn {
        index: 4,
        path: &p4.to_string_lossy(),
        start_offset_ms: 30_000,
        duration_ms: 5,
        speech_ms: -1,
    };
    parse(&app.chunk_ready_json(ID0, c));
    assert_eq!(read_sidecar(&e.export(), ID0).chunks[1].speech_ms, None);
    // Past recording, only a known chunk (a re-cut file) may be reported.
    app.finish_json(ID0, 60_000, "");
    assert_eq!(
        err(ready(&app, &e, 9, 60_000, 1000)),
        "chunk 9 is not part of this meeting"
    );
    assert_eq!(
        ready(&app, &e, 3, 0, 30_000),
        json!({"index": 3, "state": "pending", "added": false})
    );
}

#[test]
fn chunk_files_are_kept_until_their_text_is_stored_and_their_segment_closed() {
    // F15-FR8.
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    ready(&app, &e, 0, 0, 30_000);
    ready(&app, &e, 1, 30_000, 30_000);
    done(&e, 0);
    // Text stored, segment still open: kept (a crash may need it).
    app.finish_json(ID0, 60_000, "");
    assert!(chunk_path(&e, 0).exists());
    // Segment closed: the done chunk's file goes; the pending one stays.
    assert_eq!(
        parse(&app.close_segment_json(ID0, 0, 60_000)),
        json!({"ok": true})
    );
    assert!(!chunk_path(&e, 0).exists());
    assert!(chunk_path(&e, 1).exists());
    // Once the last one has its text, the folder goes too.
    done(&e, 1);
    assert_eq!(
        app.library().prune_chunks(ID0, &app.chunk_root()).unwrap(),
        1
    );
    assert!(!e.data.path().join("chunks").join(ID0).exists());
    // Pruning again is harmless.
    assert_eq!(
        app.library().prune_chunks(ID0, &app.chunk_root()).unwrap(),
        0
    );
}

#[test]
fn a_crashed_segment_is_listed_with_its_finished_chunks_to_rebuild_from() {
    // F15-FR8 / F15-AC8 (host part): the muxer was killed, so the segment file may be
    // unreadable; its chunk files are listed in order with the overlap to skip.
    let e = Env::new();
    {
        let app = e.app();
        begin(&app, T0);
        fs::write(e.export().join(format!("{ID0}-01.m4a")), b"no index").unwrap();
        ready(&app, &e, 0, 0, 35_000);
        ready(&app, &e, 1, 34_000, 30_000);
        // Chunk 2's file was never written (the cutter reported it unwritten).
        report(
            &app,
            &chunk_path(&e, 2).to_string_lossy(),
            2,
            64_000,
            30_000,
        );
    }
    let app = e.app();
    let r = parse(&app.recover_json(false));
    let seg = e.export().join(format!("{ID0}-01.m4a"));
    assert_eq!(
        r["measure"],
        json!([{"id": ID0, "index": 0, "path": seg, "chunks": [
            {"path": chunk_path(&e, 0), "skipMs": 0}, {"path": chunk_path(&e, 1), "skipMs": 1_000}]}])
    );
    assert_eq!(r["recovered"], json!([ID0]));
}

#[test]
fn deleting_a_meeting_deletes_its_chunk_folder() {
    let e = Env::new();
    let app = e.app();
    begin(&app, T0);
    ready(&app, &e, 0, 0, 30_000);
    app.finish_json(ID0, 30_000, "");
    assert_eq!(parse(&app.delete_json(ID0))["ok"], true);
    assert!(!e.data.path().join("chunks").join(ID0).exists());
}
