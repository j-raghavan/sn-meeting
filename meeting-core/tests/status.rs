//! INV8 / Data Model: the status machine, every (phase, event) pair.
use FailedAt::{Summarising as S, Transcribing as T};
use Phase as P;
use meeting::status::{Action, Event, FailedAt, Illegal, Phase, Status, job_actions, next};

/// The expected table, row by row; every (phase, event) pair not here must be Illegal.
const LEGAL: &[(Phase, Event, Phase)] = &[
    (P::Recording, Event::Stop, P::Recorded),
    (P::Recording, Event::RecoverNoService, P::Recorded),
    (P::Recorded, Event::StartTranscribe, P::Transcribing),
    (P::Transcribing, Event::AllSegmentsText, P::Transcribed),
    (P::Transcribing, Event::TranscribeFailed, P::Failed(T)),
    (P::Transcribing, Event::RecoverNoJob, P::Failed(T)),
    (P::Transcribed, Event::StartSummarise, P::Transcribed),
    (P::Transcribed, Event::SummariseOk, P::Done),
    (P::Transcribed, Event::SummariseFailed, P::Failed(S)),
    (P::Done, Event::StartSummarise, P::Done),
    (P::Done, Event::SummariseOk, P::Done),
    (P::Done, Event::SummariseFailed, P::Failed(S)),
    (P::Failed(T), Event::StartTranscribe, P::Transcribing),
    (P::Failed(S), Event::StartSummarise, P::Transcribed),
    // F5-FR13: Transcribe again clears the segment texts and starts over.
    (P::Transcribed, Event::TranscribeAgain, P::Transcribing),
    (P::Done, Event::TranscribeAgain, P::Transcribing),
];

#[test]
fn the_table_is_exhaustive() {
    let mut legal = 0;
    for from in Phase::ALL {
        for event in Event::ALL {
            let want = LEGAL.iter().find(|(f, e, _)| *f == from && *e == event);
            match want {
                Some((_, _, to)) => {
                    legal += 1;
                    assert_eq!(next(from, event), Ok(*to), "{from:?} {event:?}");
                }
                None => assert_eq!(next(from, event), Err(Illegal { from, event })),
            }
        }
    }
    assert_eq!(legal, LEGAL.len());
    assert_eq!(Phase::ALL.len() * Event::ALL.len(), 70);
}

/// INV8: every phase has a job action, or is in progress and recovery leads to one.
#[test]
fn nothing_is_stranded() {
    for p in Phase::ALL {
        match job_actions(p) {
            Some(a) => assert!(!a.is_empty()),
            None => {
                let r = next(p, Event::RecoverNoService).or(next(p, Event::RecoverNoJob));
                assert!(
                    job_actions(r.unwrap()).is_some_and(|a| !a.is_empty()),
                    "{p:?}"
                );
            }
        }
    }
}

#[test]
fn parts_round_trip_and_failed_defaults_to_transcribing() {
    for p in Phase::ALL {
        let (s, f) = p.parts();
        assert_eq!(Phase::from_parts(s, f), p);
    }
    assert_eq!(Phase::from_parts(Status::Failed, None), P::Failed(T));
    assert_eq!(Phase::from_parts(Status::Done, Some(S)), P::Done);
    let json = serde_json::to_string(&(Status::Transcribed, FailedAt::Summarising)).unwrap();
    assert_eq!(json, r#"["transcribed","summarising"]"#);
}

/// F7-FR2: Transcribe for recorded and failed(transcribing); Summarise again for transcribed,
/// done and failed(summarising), with Transcribe again for transcribed and done (F5-FR13);
/// recording and transcribing are in progress.
#[test]
fn the_meeting_screen_actions() {
    let want: [(Phase, Option<&[Action]>); 7] = [
        (P::Recording, None),
        (P::Recorded, Some(&[Action::Transcribe])),
        (P::Transcribing, None),
        (
            P::Transcribed,
            Some(&[Action::SummariseAgain, Action::TranscribeAgain]),
        ),
        (
            P::Done,
            Some(&[Action::SummariseAgain, Action::TranscribeAgain]),
        ),
        (P::Failed(T), Some(&[Action::Transcribe])),
        (P::Failed(S), Some(&[Action::SummariseAgain])),
    ];
    for (p, actions) in want {
        assert_eq!(job_actions(p), actions, "{p:?}");
    }
}

#[test]
fn illegal_pairs_are_named() {
    let illegal = Phase::ALL
        .iter()
        .flat_map(|f| Event::ALL.iter().map(move |e| next(*f, *e)))
        .filter(Result::is_err)
        .count();
    assert_eq!(illegal, 54);
    let e = next(P::Done, Event::Stop).unwrap_err();
    assert_eq!(e.to_string(), "Stop is not allowed while Done");
}
