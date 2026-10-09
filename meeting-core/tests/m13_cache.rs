//! F18-FR6 and the M14 contract: the event cache Search reads is `<filesDir>/calendar/<linkId>.json`
//! shaped `{"events": [{eventKey, title, start, location, attendees, agenda}]}`, with the
//! cache's own fields beside them, and no URL inside.
use chrono::FixedOffset;
use meeting::calendar::cache::{self, CacheFile, LinkStatus};
use meeting::calendar::ics;
use meeting::tz::FixedTz;

#[test]
fn search_reads_what_the_cache_writes() {
    let tz = FixedTz(FixedOffset::east_opt(-7 * 3600).unwrap());
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/calendar/google.ics",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let win = (1_791_183_600_000, 1_791_788_400_000); // Mon 5 – Mon 12 Oct 2026, PDT
    let x = ics::expand(&text, "0123abcd", &tz, win).unwrap();
    let d = tempfile::tempdir().unwrap();
    let c = CacheFile {
        link_id: "0123abcd".into(),
        calendar: x.name.clone(),
        fetched_at_ms: 1,
        attempted_at_ms: 1,
        status: LinkStatus::Ok,
        skipped: x.skipped,
        events: x.events.clone(),
        ..CacheFile::default()
    };
    cache::write(d.path(), &c).unwrap();
    assert_eq!(cache::read(d.path(), "0123abcd"), c);
    let docs = meeting::search::update::read_events(&cache::path(d.path(), "0123abcd"), "0123abcd");
    assert_eq!(docs.len(), x.events.len());
    let mon = &docs[0];
    assert_eq!(mon.title, "Team stand-up");
    assert_eq!(mon.start, "2026-10-05T10:00:00-07:00");
    assert_eq!(mon.location.as_deref(), Some("Room 4"));
    assert_eq!(mon.attendees[0].name.as_deref(), Some("Priya Shah"));
    assert!(mon.agenda.as_deref().unwrap().contains("Release plan"));
    let raw = std::fs::read_to_string(cache::path(d.path(), "0123abcd")).unwrap();
    for field in [
        "\"etag\"",
        "\"fetchedAtMs\"",
        "\"calendar\":\"Work\"",
        "\"endMs\"",
        "\"seriesUid\"",
    ] {
        assert!(raw.contains(field), "{field}");
    }
}
