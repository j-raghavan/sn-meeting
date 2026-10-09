//! F19-FR4 for near-live meetings: the transcript is indexed from the chunks as each lands,
//! with each line's estimated time, the overlap's repeated words once, and not again from the
//! segments their text is copied into.
use meeting::search::Kind;
use meeting::search::units::meeting_units;
use meeting::sidecar::{self, Chunk, ChunkState};

fn chunk(index: u32, start: u64, overlap: u64, state: ChunkState, text: Option<&str>) -> Chunk {
    Chunk {
        index,
        seg_index: 0,
        start_offset_ms: start,
        duration_ms: 30_000,
        overlap_ms: overlap,
        state,
        text: text.map(str::to_owned),
        attempts: 1,
        last_error: None,
        speech_ms: None,
        extra: Default::default(),
    }
}

#[test]
fn chunk_lines_are_searchable_as_they_land() {
    let mut sc = sidecar::blank("meeting-20261009-100000");
    sc.segments = vec![sidecar::Segment {
        file: "meeting-20261009-100000-01.m4a".into(),
        start_offset_ms: 0,
        duration_ms: Some(90_000),
        // Filled from the chunks (F15-FR6): not indexed a second time.
        text: Some("We ship it on Friday.\n\nNext, the budget.".into()),
        flag: None,
        extra: Default::default(),
    }];
    sc.chunks = vec![
        chunk(0, 0, 0, ChunkState::Done, Some("We ship it on Friday.")),
        chunk(
            1,
            29_000,
            1_000,
            ChunkState::Done,
            Some("on Friday. Next, the budget."),
        ),
        chunk(2, 59_000, 0, ChunkState::Missing, None),
        chunk(3, 89_000, 0, ChunkState::Pending, None),
    ];
    let u = meeting_units(&sc, &[], &[]);
    let t: Vec<(&str, &str, Option<u64>)> = u
        .iter()
        .filter(|u| u.kind == Kind::Transcript)
        .map(|u| (u.unit_id.as_str(), u.text.as_str(), u.at_ms))
        .collect();
    assert_eq!(
        t,
        [
            ("t:c0:0", "We ship it on Friday.", Some(0)),
            ("t:c1:0", "Next, the budget.", Some(30_000)),
        ]
    );
}

#[test]
fn a_meeting_without_chunks_is_indexed_from_its_segments_as_before() {
    let mut sc = sidecar::blank("meeting-20261009-100000");
    sc.segments = vec![sidecar::Segment {
        file: "meeting-20261009-100000-01.m4a".into(),
        start_offset_ms: 0,
        duration_ms: Some(10_000),
        text: Some("Hello there.".into()),
        flag: None,
        extra: Default::default(),
    }];
    let u = meeting_units(&sc, &[], &[]);
    assert_eq!(u[0].unit_id, "t:0:0");
}
