//! F16: usage records and what they cost. A record keeps the counters the provider reported, so
//! it can be priced again; it is priced once, when written, with the rate in force at its `at`
//! time (F16-FR3), and only an explicit re-price changes its `usd` (F16-FR6).
use crate::keyfile::Provider;
use crate::providers::Measured;
use crate::rates::{Rate, RateTable, Unit};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Why a record has no price.
pub const NO_RATE: &str = "no rate";
pub const NO_USAGE: &str = "no usage reported";
pub const NO_DATE: &str = "no date";

/// What the call was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Transcribe,
    Summarise,
    /// A handwritten search query read by the summarise provider (F19-FR9); no meeting.
    Query,
}

/// Where a priced quantity came from: the provider's own report, or the audio's measured
/// length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Provider,
    Duration,
}

/// The entry a record was priced with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateRef {
    pub provider: Provider,
    pub model: String,
    pub as_of: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
}

/// One entry of a sidecar's `usage` list (spec Data Model). Unknown fields are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    /// RFC 3339 with the device's offset, when the call ended.
    pub at: String,
    pub kind: Kind,
    pub provider: Provider,
    pub model: String,
    /// The near-live chunk transcribed (F15); `null` for a segment or a summary.
    #[serde(default)]
    pub chunk: Option<u32>,
    /// The segment transcribed, 0-based (F5's per-segment path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<usize>,
    #[serde(default)]
    pub audio_ms: Option<u64>,
    /// How the rate bills: `minute` or `tokens`; from the counters when there is no rate.
    #[serde(default)]
    pub billing: Option<Unit>,
    /// The seconds priced: the provider's when reported, else the audio's length.
    #[serde(default)]
    pub billed_seconds: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub audio_input_tokens: Option<u64>,
    #[serde(default)]
    pub text_input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    /// OpenAI transcription's `usage.type` and `usage.seconds`, as reported, for re-pricing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_seconds: Option<f64>,
    /// USD, 6 decimals; `null` with a `reason` when it can't be priced.
    #[serde(default)]
    pub usd: Option<f64>,
    #[serde(default)]
    pub rate: Option<RateRef>,
    #[serde(default)]
    pub source: Option<Source>,
    /// Priced from the audio's length with a per-minute estimate (no tokens reported).
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One entry of a sidecar's `usage` as read: a record, or anything else (a hand edit, a shape a
/// newer build writes), kept as written and counted as unpriced, so it never makes the meeting
/// unreadable (F16-FR8).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum UsageEntry {
    Record(Box<UsageRecord>),
    Unreadable(Value),
}

impl From<UsageRecord> for UsageEntry {
    fn from(r: UsageRecord) -> UsageEntry {
        UsageEntry::Record(Box::new(r))
    }
}

impl From<Value> for UsageEntry {
    fn from(v: Value) -> UsageEntry {
        match serde_json::from_value::<UsageRecord>(v.clone()) {
            Ok(r) => r.into(),
            Err(_) => UsageEntry::Unreadable(v),
        }
    }
}

impl<'de> Deserialize<'de> for UsageEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Value::deserialize(d)?.into())
    }
}

impl UsageEntry {
    pub fn record(&self) -> Option<&UsageRecord> {
        match self {
            UsageEntry::Record(r) => Some(r),
            UsageEntry::Unreadable(_) => None,
        }
    }

    pub fn record_mut(&mut self) -> Option<&mut UsageRecord> {
        match self {
            UsageEntry::Record(r) => Some(r),
            UsageEntry::Unreadable(_) => None,
        }
    }
}

/// `usage` as read: a list of entries; any other value is kept as one unreadable entry.
pub fn lenient_usage<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<UsageEntry>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a.into_iter().map(UsageEntry::from).collect(),
        Value::Null => Vec::new(),
        other => vec![UsageEntry::Unreadable(other)],
    })
}

/// A fresh record id: `u-` and 8 hex digits of a random (v4) UUID.
pub fn new_usage_id() -> String {
    format!("u-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

/// USD to 6 decimals, as stored.
pub fn round6(usd: f64) -> f64 {
    (usd * 1_000_000.0).round() / 1_000_000.0
}

impl UsageRecord {
    /// The record for one call that ended at `at`, priced with `rates` (F16-FR3).
    pub fn new(kind: Kind, m: &Measured, at: String, rates: &RateTable) -> UsageRecord {
        let mut r = UsageRecord {
            id: new_usage_id(),
            at,
            kind,
            provider: m.provider,
            model: m.model.clone(),
            chunk: None,
            segment: None,
            audio_ms: m.audio_ms,
            billing: None,
            billed_seconds: None,
            input_tokens: m.input_tokens,
            audio_input_tokens: m.audio_input_tokens,
            text_input_tokens: m.text_input_tokens,
            cached_input_tokens: m.cached_input_tokens,
            cache_write_tokens: m.cache_write_tokens,
            output_tokens: m.output_tokens,
            reasoning_tokens: m.reasoning_tokens,
            usage_type: m.usage_type.clone(),
            reported_seconds: m.seconds,
            usd: None,
            rate: None,
            source: None,
            estimated: false,
            reason: None,
            extra: Map::new(),
        };
        r.price(rates);
        r
    }

    fn has_tokens(&self) -> bool {
        self.input_tokens.is_some() || self.output_tokens.is_some()
    }

    /// The local day of `at` (its own offset), which picks the rate in force.
    pub fn day(&self) -> Option<NaiveDate> {
        DateTime::parse_from_rfc3339(&self.at)
            .ok()
            .map(|t| t.date_naive())
    }

    /// Price (or re-price) from the stored counters with `rates`. Every derived field is set
    /// afresh; the counters are never changed.
    pub fn price(&mut self, rates: &RateTable) {
        self.usd = None;
        self.rate = None;
        self.source = None;
        self.estimated = false;
        self.reason = None;
        self.billed_seconds = self.reported_seconds;
        self.billing = if self.has_tokens() {
            Some(Unit::Tokens)
        } else if self.reported_seconds.is_some() || self.audio_ms.is_some() {
            Some(Unit::Minute)
        } else {
            None
        };
        let Some(day) = self.day() else {
            self.reason = Some(NO_DATE.into());
            return;
        };
        let Some(rate) = rates.lookup(self.provider, &self.model, day) else {
            self.reason = Some(NO_RATE.into());
            return;
        };
        self.billing = Some(rate.unit);
        let audio_s = self.audio_ms.map(|ms| ms as f64 / 1000.0);
        let priced = match rate.unit {
            Unit::Minute => {
                let secs = self
                    .reported_seconds
                    .map(|s| (s, Source::Provider))
                    .or(audio_s.map(|s| (s, Source::Duration)));
                secs.zip(rate.per_minute).map(|((s, src), pm)| {
                    self.billed_seconds = Some(s);
                    (s / 60.0 * pm, src)
                })
            }
            Unit::Tokens if self.has_tokens() => Some((self.token_cost(rate), Source::Provider)),
            Unit::Tokens => audio_s.zip(rate.per_minute_estimate).map(|(s, pm)| {
                self.billed_seconds = Some(s);
                self.estimated = true;
                (s / 60.0 * pm, Source::Duration)
            }),
        };
        let Some((usd, source)) = priced else {
            self.reason = Some(NO_USAGE.into());
            return;
        };
        self.usd = Some(round6(usd));
        self.source = Some(source);
        self.rate = Some(RateRef {
            provider: rate.provider,
            model: rate.model.clone(),
            as_of: rate.as_of.clone(),
            valid_from: rate.valid_from.clone(),
        });
    }

    /// `uncached × input + audio × audioInput + cached × cachedInput + cacheWrite × cacheWrite
    /// + output × output`, per 1M tokens. A price the entry leaves out is its `input` price.
    fn token_cost(&self, rate: &Rate) -> f64 {
        let input_price = rate.input.unwrap_or(0.0);
        let input = self.input_tokens.unwrap_or(0);
        let cached = self.cached_input_tokens.unwrap_or(0);
        let write = self.cache_write_tokens.unwrap_or(0);
        let uncached = input.saturating_sub(cached.saturating_add(write));
        let (audio, audio_price) = match (rate.audio_input, self.audio_input_tokens) {
            (Some(p), Some(a)) => (a.min(uncached), p),
            _ => (0, input_price),
        };
        let text = uncached - audio;
        let per = |n: u64, p: f64| n as f64 * p;
        let total = per(text, input_price)
            + per(audio, audio_price)
            + per(cached, rate.cached_input.unwrap_or(input_price))
            + per(write, rate.cache_write.unwrap_or(input_price))
            + per(self.output_tokens.unwrap_or(0), rate.output.unwrap_or(0.0));
        total / 1_000_000.0
    }
}

/// A meeting's (or a month's) spend: the sum of the priced records, and whether any record
/// could not be priced (shown as "+").
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Total {
    pub usd: f64,
    /// Records with `usd: null`.
    pub unpriced: usize,
    pub records: usize,
    /// "≈ $0.41", "≈ $0.41+", "< $0.01", or "—" with no records (F16-FR5).
    pub label: String,
}

impl Total {
    /// The total of `entries`; an unreadable entry counts as unpriced.
    pub fn of<'a>(entries: impl IntoIterator<Item = &'a UsageEntry>) -> Total {
        let mut t = Total::default();
        for e in entries {
            t.records += 1;
            match e.record().and_then(|r| r.usd) {
                Some(u) if u.is_finite() => t.usd += u,
                _ => t.unpriced += 1,
            }
        }
        t.usd = round6(t.usd);
        t.label = label(&t);
        t
    }
}

/// An amount rounded to cents: "$3.20", or "< $0.01" for less than a cent but more than none.
pub fn money(usd: f64) -> String {
    if usd > 0.0 && usd < 0.005 {
        "< $0.01".to_owned()
    } else {
        format!("${usd:.2}")
    }
}

/// F16-FR5's meeting label: "—" with no records, else "≈ $0.41", with "+" when a record has no
/// price.
pub fn label(t: &Total) -> String {
    if t.records == 0 {
        return "—".to_owned();
    }
    let plus = if t.unpriced > 0 { "+" } else { "" };
    match money(t.usd) {
        m if m.starts_with('<') => format!("{m}{plus}"),
        m => format!("≈ {m}{plus}"),
    }
}

/// A meeting's total over all its records.
pub fn meeting_total(sc: &crate::sidecar::Sidecar) -> Total {
    Total::of(&sc.usage)
}

/// One provider's spend in a month, by kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ByKind {
    pub transcribe: f64,
    pub summarise: f64,
}

/// One month of `nativeSpend`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    /// `YYYY-MM`.
    pub month: String,
    pub usd: f64,
    /// Meetings with at least one record this month.
    pub meetings: usize,
    pub unpriced: usize,
    /// Home's line: "October · $3.20 spent · 6 meetings", or "October · nothing spent yet".
    pub label: String,
    pub by_provider: std::collections::BTreeMap<String, ByKind>,
    /// F19-FR9: handwritten search queries this month ("Search" in Settings → SPEND), in `usd`.
    pub search: f64,
    pub searches: usize,
}

/// F16-FR7: spend is per device. It is read from the sidecars in this device's current and
/// previous save folders; there is no account, server or sync, and records carry no device id
/// (OQ17), so a sidecar copied in from another device counts here too.
pub const NOTE_DEVICE: &str = "Counted from the meetings in this device's save folders";
/// OQ16: a request that failed without a reply reports no usage, so it is not counted.
pub const NOTE_FAILED: &str = "Failed requests aren't counted";

/// `nativeSpend`'s reply.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub months: Vec<Month>,
    /// Records in these months with no price.
    pub unpriced: usize,
    /// What the totals do and don't include, for Settings → SPEND.
    pub notes: Vec<&'static str>,
}

/// The most months one report covers (ten years).
pub const MAX_MONTHS: usize = 120;

fn parse_month(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
        .ok()
        .filter(|_| s.len() == 7)
}

/// The record's month in the device's time zone, `YYYY-MM`.
fn month_of(r: &UsageRecord, tz: &dyn crate::tz::LocalTz) -> Option<String> {
    let t = DateTime::parse_from_rfc3339(&r.at).ok()?;
    let local = crate::tz::local_at(tz, t.timestamp_millis())?;
    Some(local.format("%Y-%m").to_string())
}

fn home_label(first: NaiveDate, usd: f64, unpriced: usize, meetings: usize) -> String {
    let name = first.format("%B");
    if meetings == 0 && usd == 0.0 && unpriced == 0 {
        return format!("{name} · nothing spent yet");
    }
    let plus = if unpriced > 0 { "+" } else { "" };
    let n = if meetings == 1 { "meeting" } else { "meetings" };
    format!("{name} · {}{plus} spent · {meetings} {n}", money(usd))
}

/// F16-FR5 / F16-FR7: spend per month from `from` to `to` (`YYYY-MM`, inclusive, every month
/// listed), from the records of `meetings` (every sidecar this device lists), bucketed by the
/// device's local month.
pub fn report(
    meetings: &[crate::sidecar::Sidecar],
    tz: &dyn crate::tz::LocalTz,
    from: &str,
    to: &str,
) -> Result<Report, String> {
    report_with(meetings, &[], tz, from, to)
}

/// `report`, also counting `searches`: the records of calls with no meeting (F19-FR9).
pub fn report_with(
    meetings: &[crate::sidecar::Sidecar],
    searches: &[UsageEntry],
    tz: &dyn crate::tz::LocalTz,
    from: &str,
    to: &str,
) -> Result<Report, String> {
    let (Some(a), Some(b)) = (parse_month(from), parse_month(to)) else {
        return Err("months must be YYYY-MM".into());
    };
    if a > b {
        return Err("fromMonth is after toMonth".into());
    }
    let mut months = Vec::new();
    let mut d = a;
    while d <= b {
        if months.len() == MAX_MONTHS {
            return Err(format!("at most {MAX_MONTHS} months"));
        }
        months.push(d);
        d = d
            .checked_add_months(chrono::Months::new(1))
            .ok_or("months out of range")?;
    }
    let mut out = Vec::new();
    let mut unpriced_all = 0;
    for first in months {
        let key = first.format("%Y-%m").to_string();
        let mut usd = 0.0;
        let mut unpriced = 0;
        let mut n = 0;
        let mut by: std::collections::BTreeMap<String, ByKind> = Default::default();
        for sc in meetings {
            let mut any = false;
            for r in sc
                .usage
                .iter()
                .filter_map(UsageEntry::record)
                .filter(|r| month_of(r, tz).as_deref() == Some(&key))
            {
                any = true;
                let Some(u) = r.usd.filter(|u| u.is_finite()) else {
                    unpriced += 1;
                    continue;
                };
                usd += u;
                let slot = by.entry(r.provider.id().to_owned()).or_default();
                match r.kind {
                    Kind::Transcribe => slot.transcribe += u,
                    Kind::Summarise | Kind::Query => slot.summarise += u,
                }
            }
            n += usize::from(any);
        }
        let (mut search, mut count) = (0.0, 0);
        for r in searches
            .iter()
            .filter_map(UsageEntry::record)
            .filter(|r| month_of(r, tz).as_deref() == Some(&key))
        {
            count += 1;
            match r.usd.filter(|u| u.is_finite()) {
                Some(u) => search += u,
                None => unpriced += 1,
            }
        }
        usd += search;
        for k in by.values_mut() {
            k.transcribe = round6(k.transcribe);
            k.summarise = round6(k.summarise);
        }
        let usd = round6(usd);
        unpriced_all += unpriced;
        out.push(Month {
            label: home_label(first, usd, unpriced, n),
            month: key,
            usd,
            meetings: n,
            unpriced,
            by_provider: by,
            search: round6(search),
            searches: count,
        });
    }
    Ok(Report {
        months: out,
        unpriced: unpriced_all,
        notes: vec![NOTE_DEVICE, NOTE_FAILED],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AT: &str = "2026-10-09T10:04:31-07:00";

    fn measured(p: Provider, model: &str) -> Measured {
        Measured::new(p, model)
    }

    fn table(extra: Vec<Value>) -> RateTable {
        RateTable::effective(&extra)
    }

    /// F16-AC1: 30 reported seconds of whisper-1 at $0.006 a minute.
    #[test]
    fn minute_billing_uses_the_reported_seconds() {
        let m = Measured {
            seconds: Some(30.0),
            audio_ms: Some(29_480),
            usage_type: Some("duration".into()),
            ..measured(Provider::OpenAi, "whisper-1")
        };
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!(r.billed_seconds, Some(30.0));
        assert_eq!(r.usd, Some(0.003));
        assert_eq!(r.source, Some(Source::Provider));
        assert_eq!(r.billing, Some(Unit::Minute));
        assert_eq!(r.rate.as_ref().unwrap().as_of, crate::rates::DEFAULTS_AS_OF);
        assert!(r.id.starts_with("u-") && r.id.len() == 10, "{}", r.id);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["source"], "provider");
        assert_eq!(v["usd"], 0.003);
        assert_eq!(
            v["chunk"],
            Value::Null,
            "known fields are written, null included"
        );
        assert!(v.get("segment").is_none());
    }

    #[test]
    fn minute_billing_falls_back_to_the_audio_length() {
        let m = Measured {
            audio_ms: Some(90_000),
            ..measured(Provider::OpenAi, "gpt-transcribe")
        };
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!((r.usd, r.source), (Some(0.00675), Some(Source::Duration)));
        assert_eq!(r.billed_seconds, Some(90.0));
        assert!(!r.estimated);
    }

    /// F16-AC2: audio and text tokens priced separately.
    #[test]
    fn audio_tokens_use_the_audio_price() {
        let rate = json!({"provider": "openai", "model": "gpt-4o-transcribe", "unit": "tokens",
            "input": 2.0, "audioInput": 6.0, "output": 10.0, "asOf": "2026-10-09", "source": "t"});
        let m = Measured {
            input_tokens: Some(500),
            audio_input_tokens: Some(480),
            text_input_tokens: Some(20),
            output_tokens: Some(90),
            ..measured(Provider::OpenAi, "gpt-4o-transcribe")
        };
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![rate]));
        // 480 × 6 + 20 × 2 + 90 × 10 = 3 820 per 1M.
        assert_eq!(r.usd, Some(0.00382));
        assert_eq!(
            (r.billing, r.source),
            (Some(Unit::Tokens), Some(Source::Provider))
        );
        // With no audio price, every input token is an input token: 500 × 2.5 + 90 × 10.
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!(r.usd, Some(0.00215));
    }

    #[test]
    fn cached_and_cache_write_tokens_have_their_own_prices() {
        let m = Measured {
            input_tokens: Some(1_500),
            cached_input_tokens: Some(300),
            cache_write_tokens: Some(200),
            output_tokens: Some(400),
            ..measured(Provider::Anthropic, "claude-opus-5-5")
        };
        let r = UsageRecord::new(Kind::Summarise, &m, AT.into(), &table(vec![]));
        // 1 000 × 4 + 300 × 0.2 + 200 × 5 + 400 × 20 = 13 060 per 1M.
        assert_eq!(r.usd, Some(0.01306));
        // A rate with no cached price charges cached tokens at the input price.
        let plain = json!({"provider": "anthropic", "model": "claude-opus-5-5",
            "unit": "tokens", "input": 4, "output": 20, "asOf": "2026-10-09", "source": "t"});
        let r = UsageRecord::new(Kind::Summarise, &m, AT.into(), &table(vec![plain]));
        assert_eq!(r.usd, Some(0.014));
    }

    /// EC19: a token-billed model with no counts falls back to the per-minute estimate.
    #[test]
    fn no_tokens_reported_is_an_estimate_from_the_audio() {
        let m = Measured {
            audio_ms: Some(60_000),
            ..measured(Provider::OpenAi, "gpt-4o-mini-transcribe")
        };
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!((r.usd, r.estimated), (Some(0.003), true));
        assert_eq!(r.source, Some(Source::Duration));
        // A summary with no counters can't be priced.
        let m = measured(Provider::OpenAi, "gpt-4o-mini");
        let r = UsageRecord::new(Kind::Summarise, &m, AT.into(), &table(vec![]));
        assert_eq!((r.usd, r.reason.as_deref()), (None, Some(NO_USAGE)));
        assert_eq!(r.billing, Some(Unit::Tokens), "the rate's unit");
        assert!(r.rate.is_none());
        // Nor can a minute-billed call with no length at all.
        let m = measured(Provider::OpenAi, "whisper-1");
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!(r.reason.as_deref(), Some(NO_USAGE));
    }

    /// F16-AC5 (the record): no rate, `usd: null`, counters kept; a rate added prices it.
    #[test]
    fn no_rate_keeps_the_counters_for_later() {
        let m = Measured {
            input_tokens: Some(1_000_000),
            output_tokens: Some(0),
            ..measured(Provider::OpenAi, "gpt-9")
        };
        let mut r = UsageRecord::new(Kind::Summarise, &m, AT.into(), &table(vec![]));
        assert_eq!((r.usd, r.reason.as_deref()), (None, Some(NO_RATE)));
        assert_eq!(r.input_tokens, Some(1_000_000));
        assert_eq!(r.billing, Some(Unit::Tokens), "from the counters");
        let rate = json!({"provider": "openai", "model": "gpt-9", "unit": "tokens",
            "input": 0.41, "output": 1, "asOf": "2026-10-09", "source": "t"});
        r.price(&table(vec![rate]));
        assert_eq!((r.usd, r.reason.clone()), (Some(0.41), None));
        let m = Measured {
            audio_ms: Some(1),
            ..measured(Provider::OpenAi, "x")
        };
        let r = UsageRecord::new(Kind::Transcribe, &m, AT.into(), &table(vec![]));
        assert_eq!(r.billing, Some(Unit::Minute));
        let r = UsageRecord::new(
            Kind::Transcribe,
            &measured(Provider::OpenAi, "x"),
            AT.into(),
            &table(vec![]),
        );
        assert_eq!(r.billing, None);
    }

    #[test]
    fn the_rate_in_force_on_the_records_day_is_used() {
        let m = Measured {
            input_tokens: Some(1_000_000),
            output_tokens: Some(0),
            ..measured(Provider::Gemini, "gemini-3.8-flash")
        };
        let t = table(vec![]);
        let before = UsageRecord::new(Kind::Summarise, &m, "2026-12-31T23:59:00-08:00".into(), &t);
        let after = UsageRecord::new(Kind::Summarise, &m, "2027-01-01T00:01:00-08:00".into(), &t);
        assert_eq!((before.usd, after.usd), (Some(0.75), Some(1.5)));
        assert_eq!(
            after.rate.unwrap().valid_from.as_deref(),
            Some("2027-01-01")
        );
        let bad = UsageRecord::new(Kind::Summarise, &m, "yesterday".into(), &t);
        assert_eq!((bad.usd, bad.reason.as_deref()), (None, Some(NO_DATE)));
    }

    #[test]
    fn records_round_trip_with_unknown_fields() {
        let mut v = serde_json::to_value(UsageRecord::new(
            Kind::Transcribe,
            &measured(Provider::OpenAi, "whisper-1"),
            AT.into(),
            &table(vec![]),
        ))
        .unwrap();
        v["future"] = json!({"x": 1});
        let r: UsageRecord = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&r).unwrap(), v);
        assert_eq!(round6(0.123_456_789), 0.123_457);
    }

    fn rec(at: &str, p: Provider, kind: Kind, usd: Option<f64>) -> UsageRecord {
        let mut r = UsageRecord::new(kind, &measured(p, "m"), at.into(), &table(vec![]));
        r.usd = usd;
        r
    }

    fn sc(id: &str, usage: Vec<UsageRecord>) -> crate::sidecar::Sidecar {
        crate::sidecar::Sidecar {
            usage: usage.into_iter().map(UsageEntry::from).collect(),
            ..crate::sidecar::blank(id)
        }
    }

    /// F16-AC5 (the label): an unpriced record shows "+".
    #[test]
    fn meeting_labels() {
        let at = AT;
        let t = |v: Vec<Option<f64>>| {
            let recs: Vec<_> = v
                .into_iter()
                .map(|u| rec(at, Provider::OpenAi, Kind::Transcribe, u).into())
                .collect();
            Total::of(&recs).label
        };
        assert_eq!(t(vec![]), "—");
        assert_eq!(t(vec![Some(0.2), Some(0.21)]), "≈ $0.41");
        assert_eq!(t(vec![Some(0.41), None]), "≈ $0.41+");
        assert_eq!(t(vec![Some(0.004)]), "< $0.01");
        assert_eq!(t(vec![Some(0.004), None]), "< $0.01+");
        assert_eq!(t(vec![None]), "≈ $0.00+");
        assert_eq!(t(vec![Some(0.076)]), "≈ $0.08");
        let total = Total::of(&[rec(at, Provider::OpenAi, Kind::Transcribe, Some(0.1)).into()]);
        assert_eq!((total.usd, total.records, total.unpriced), (0.1, 1, 0));
        assert_eq!(
            meeting_total(&sc("meeting-20261009-100000", vec![])).label,
            "—"
        );
    }

    /// F16-AC6: records either side of midnight on 30 September, local time.
    #[test]
    fn months_split_at_local_midnight() {
        let tz = crate::tz::FixedTz(chrono::FixedOffset::west_opt(7 * 3600).unwrap());
        let a = rec(
            "2026-09-30T23:50:00-07:00",
            Provider::OpenAi,
            Kind::Transcribe,
            Some(0.5),
        );
        let b = rec(
            "2026-10-01T00:10:00-07:00",
            Provider::Gemini,
            Kind::Summarise,
            Some(0.25),
        );
        // Stored with another offset, but 23:50 local all the same.
        let c = rec(
            "2026-10-01T06:50:00Z",
            Provider::OpenAi,
            Kind::Summarise,
            None,
        );
        let meetings = [
            sc("meeting-20260930-235000", vec![a, c]),
            sc("meeting-20261001-000500", vec![b]),
            sc("meeting-20261002-000500", vec![]),
        ];
        let r = report(&meetings, &tz, "2026-09", "2026-10").unwrap();
        let [sep, oct] = &r.months[..] else {
            panic!("{r:?}")
        };
        assert_eq!(
            (sep.month.as_str(), sep.usd, sep.meetings, sep.unpriced),
            ("2026-09", 0.5, 1, 1)
        );
        assert_eq!(
            (oct.month.as_str(), oct.usd, oct.meetings),
            ("2026-10", 0.25, 1)
        );
        assert_eq!(oct.label, "October · $0.25 spent · 1 meeting");
        assert_eq!(sep.label, "September · $0.50+ spent · 1 meeting");
        assert_eq!(
            sep.by_provider["openai"],
            ByKind {
                transcribe: 0.5,
                summarise: 0.0
            }
        );
        assert_eq!(
            oct.by_provider["gemini"],
            ByKind {
                transcribe: 0.0,
                summarise: 0.25
            }
        );
        assert_eq!(r.unpriced, 1);
        let r = report(&meetings, &tz, "2026-11", "2027-01").unwrap();
        assert_eq!(r.months.len(), 3);
        assert_eq!(r.months[0].label, "November · nothing spent yet");
        let two = [
            sc(
                "meeting-20261001-000500",
                vec![rec(
                    "2026-10-02T00:00:00-07:00",
                    Provider::OpenAi,
                    Kind::Transcribe,
                    Some(1.0),
                )],
            ),
            sc(
                "meeting-20261003-000500",
                vec![rec(
                    "2026-10-04T00:00:00-07:00",
                    Provider::OpenAi,
                    Kind::Transcribe,
                    Some(2.2),
                )],
            ),
        ];
        let r = report(&two, &tz, "2026-10", "2026-10").unwrap();
        assert_eq!(r.months[0].label, "October · $3.20 spent · 2 meetings");
    }

    #[test]
    fn bad_month_ranges_are_refused() {
        let tz = crate::tz::FixedTz(chrono::FixedOffset::east_opt(0).unwrap());
        let r = |a, b| report(&[], &tz, a, b).unwrap_err();
        assert_eq!(r("2026-13", "2026-12"), "months must be YYYY-MM");
        assert_eq!(r("2026-1", "2026-12"), "months must be YYYY-MM");
        assert_eq!(r("2026-12", "2026-01"), "fromMonth is after toMonth");
        assert_eq!(r("2000-01", "2026-01"), "at most 120 months");
        assert!(report(&[], &tz, "2026-01", "2035-12").is_ok());
        assert_eq!(money(0.0), "$0.00");
    }
}
