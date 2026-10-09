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
}
