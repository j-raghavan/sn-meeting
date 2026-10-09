//! Device local time, injectable for tests: meeting ids, `startedAt`, legacy filename stamps.
use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, Offset, SecondsFormat, TimeZone};
use std::fmt;

pub trait LocalTz: Send + Sync + fmt::Debug {
    /// The local UTC offset at instant `utc_ms`.
    fn offset_at_utc(&self, utc_ms: i64) -> FixedOffset;
    /// The offset for a local wall-clock time: the earlier one when ambiguous (DST fall-back),
    /// the pre-transition one inside a DST gap.
    fn offset_at_local(&self, local: NaiveDateTime) -> FixedOffset;
}

/// `chrono::Local`. DEVICE-UNVERIFIED: on Android chrono reads the zone from
/// `persist.sys.timezone` and the system tzdata; check the device's zone in M7.
#[derive(Debug, Clone, Copy)]
pub struct DeviceTz;

impl LocalTz for DeviceTz {
    fn offset_at_utc(&self, utc_ms: i64) -> FixedOffset {
        let utc = DateTime::from_timestamp_millis(utc_ms).unwrap_or_default();
        chrono::Local
            .offset_from_utc_datetime(&utc.naive_utc())
            .fix()
    }
    fn offset_at_local(&self, local: NaiveDateTime) -> FixedOffset {
        let found = chrono::Local
            .offset_from_local_datetime(&local)
            .map(|o| o.fix());
        choose(found, || {
            chrono::Local.offset_from_utc_datetime(&local).fix()
        })
    }
}

/// The offset rule for a local time: the only one, the earlier of two, or (in a gap) `gap()`.
pub fn choose(found: LocalResult<FixedOffset>, gap: impl FnOnce() -> FixedOffset) -> FixedOffset {
    match found {
        LocalResult::Single(o) | LocalResult::Ambiguous(o, _) => o,
        LocalResult::None => gap(),
    }
}

/// A fixed offset, for tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedTz(pub FixedOffset);

impl LocalTz for FixedTz {
    fn offset_at_utc(&self, _: i64) -> FixedOffset {
        self.0
    }
    fn offset_at_local(&self, _: NaiveDateTime) -> FixedOffset {
        self.0
    }
}

/// `utc_ms` as local time with its offset; `None` outside chrono's range.
pub fn local_at(tz: &dyn LocalTz, utc_ms: i64) -> Option<DateTime<FixedOffset>> {
    Some(DateTime::from_timestamp_millis(utc_ms)?.with_timezone(&tz.offset_at_utc(utc_ms)))
}

/// A wall-clock local time as epoch ms (legacy filename stamps, F3-FR4).
pub fn local_to_utc_ms(tz: &dyn LocalTz, local: NaiveDateTime) -> i64 {
    let off = tz.offset_at_local(local);
    local.and_utc().timestamp_millis() - i64::from(off.local_minus_utc()) * 1000
}

/// `startedAt`'s form: RFC 3339 to the second with the offset, e.g. `2026-10-07T21:17:06-07:00`.
pub fn rfc3339(t: &DateTime<FixedOffset>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off(h: i32) -> FixedOffset {
        FixedOffset::east_opt(h * 3600).unwrap()
    }

    #[test]
    fn fixed_offsets_convert_both_ways() {
        let tz = FixedTz(off(-7));
        let t = local_at(&tz, 1_791_433_026_000).unwrap();
        assert_eq!(rfc3339(&t), "2026-10-07T21:17:06-07:00");
        assert_eq!(local_to_utc_ms(&tz, t.naive_local()), 1_791_433_026_000);
        assert!(local_at(&tz, i64::MAX).is_none());
        let utc = FixedTz(off(0));
        assert_eq!(
            rfc3339(&local_at(&utc, 0).unwrap()),
            "1970-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn the_dst_rule() {
        assert_eq!(choose(LocalResult::Single(off(1)), || off(9)), off(1));
        assert_eq!(
            choose(LocalResult::Ambiguous(off(2), off(1)), || off(9)),
            off(2)
        );
        assert_eq!(choose(LocalResult::None, || off(9)), off(9));
    }

    #[test]
    fn the_device_zone_agrees_with_chrono_local() {
        let tz = DeviceTz;
        let ms = 1_791_433_026_000;
        let want = chrono::Local
            .timestamp_millis_opt(ms)
            .unwrap()
            .offset()
            .fix();
        assert_eq!(tz.offset_at_utc(ms), want);
        let local = local_at(&tz, ms).unwrap().naive_local();
        assert_eq!(local_to_utc_ms(&tz, local), ms);
        assert_eq!(
            tz.offset_at_utc(i64::MAX),
            tz.offset_at_utc(0),
            "out of range: the epoch's"
        );
    }
}
