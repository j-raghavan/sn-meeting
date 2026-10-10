//! F16-FR4: the rates table. Defaults are compiled in with their sources and the date the pricing
//! pages were read; the user's edits live in `settings.json` under `rates` and replace the default
//! with the same provider, model and `validFrom`. Rates are plain numbers, never secrets, and are
//! never fetched from the network.
use crate::keyfile::Provider;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// When the providers' pricing pages behind the defaults were read.
pub const DEFAULTS_AS_OF: &str = "2026-10-09";
const OPENAI_PRICING: &str = "https://developers.openai.com/api/docs/pricing";
/// GPT-5's model pages (checked 2026-10-09; the page marks gpt-5 deprecated, still priced).
const GPT5_PRICING: &str = "https://developers.openai.com/api/docs/models/gpt-5";
const GPT5_MINI_PRICING: &str = "https://developers.openai.com/api/docs/models/gpt-5-mini";
const GEMINI_PRICING: &str = "https://ai.google.dev/gemini-api/docs/pricing";
const ANTHROPIC_PRICING: &str = "https://platform.claude.com/docs/en/about-claude/pricing";
/// Where an edited entry says it came from.
pub const USER_SOURCE: &str = "edited in Settings";
/// No price is anywhere near this; a bigger number is a typo.
const MAX_PRICE: f64 = 10_000.0;
const MAX_MODEL_LEN: usize = 128;
const MAX_TEXT: usize = 300;

/// How a model is billed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Per minute of audio (`perMinute`).
    Minute,
    /// Per 1M tokens (`input`, `output`, and optionally `cachedInput`, `cacheWrite`,
    /// `audioInput`).
    Tokens,
}

/// One rates entry. Prices are USD: per minute, or per 1M tokens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rate {
    pub provider: Provider,
    /// An exact model id, or a prefix of the ids it prices (`gpt-4o-mini` prices
    /// `gpt-4o-mini-2024-07-18`).
    pub model: String,
    pub unit: Unit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<f64>,
    /// A token-billed model's price per minute, used only when a reply reports no tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute_estimate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    /// `YYYY-MM-DD`: the first day this price applies; absent = always.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    /// `YYYY-MM-DD`: when this price was read or edited.
    pub as_of: String,
    pub source: String,
    /// A caution shown beside the entry ("check this rate").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Where an effective entry came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RateOrigin {
    Default,
    User,
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn text_ok(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT && !s.chars().any(char::is_control)
}

impl Rate {
    fn base(provider: Provider, model: &str, unit: Unit, source: &str) -> Rate {
        Rate {
            provider,
            model: model.to_owned(),
            unit,
            per_minute: None,
            per_minute_estimate: None,
            input: None,
            cached_input: None,
            cache_write: None,
            audio_input: None,
            output: None,
            valid_from: None,
            as_of: DEFAULTS_AS_OF.to_owned(),
            source: source.to_owned(),
            note: None,
        }
    }

    fn minute(provider: Provider, model: &str, per_minute: f64, source: &str) -> Rate {
        Rate {
            per_minute: Some(per_minute),
            ..Rate::base(provider, model, Unit::Minute, source)
        }
    }

    fn tokens(provider: Provider, model: &str, input: f64, output: f64, source: &str) -> Rate {
        Rate {
            input: Some(input),
            output: Some(output),
            ..Rate::base(provider, model, Unit::Tokens, source)
        }
    }

    /// The entry's identity: a user entry with the same one replaces the default.
    pub fn key(&self) -> (Provider, &str, Option<&str>) {
        (self.provider, &self.model, self.valid_from.as_deref())
    }

    /// Whether `other` charges the same: unit and every price.
    pub fn same_prices(&self, other: &Rate) -> bool {
        self.unit == other.unit && self.prices() == other.prices()
    }

    fn prices(&self) -> [Option<f64>; 7] {
        [
            self.per_minute,
            self.per_minute_estimate,
            self.input,
            self.cached_input,
            self.cache_write,
            self.audio_input,
            self.output,
        ]
    }

    /// The first problem with the entry, as `(field, message)`.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        let m = &self.model;
        if m.is_empty()
            || m.len() > MAX_MODEL_LEN
            || m.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(("model", "model name is invalid".into()));
        }
        let names = [
            "perMinute",
            "perMinuteEstimate",
            "input",
            "cachedInput",
            "cacheWrite",
            "audioInput",
            "output",
        ];
        for (name, p) in names.iter().zip(self.prices()) {
            if p.is_some_and(|p| !(p.is_finite() && (0.0..=MAX_PRICE).contains(&p))) {
                return Err((
                    name,
                    format!("{name} must be a price from 0 to {MAX_PRICE}"),
                ));
            }
        }
        match self.unit {
            Unit::Minute if self.per_minute.is_none() => {
                return Err(("perMinute", "a per-minute rate needs perMinute".into()));
            }
            Unit::Tokens if self.input.is_none() => {
                return Err(("input", "a token rate needs input".into()));
            }
            Unit::Tokens if self.output.is_none() => {
                return Err(("output", "a token rate needs output".into()));
            }
            _ => {}
        }
        if self
            .valid_from
            .as_deref()
            .is_some_and(|d| date(d).is_none())
        {
            return Err(("validFrom", "validFrom must be YYYY-MM-DD".into()));
        }
        if date(&self.as_of).is_none() {
            return Err(("asOf", "asOf must be YYYY-MM-DD".into()));
        }
        if !text_ok(&self.source) || !self.note.as_deref().is_none_or(text_ok) {
            return Err(("source", "text is too long".into()));
        }
        Ok(())
    }
}

/// The rates compiled into the core, read from the providers' pricing pages on
/// [`DEFAULTS_AS_OF`]. Owner-editable in Settings.
pub fn defaults() -> Vec<Rate> {
    use Provider::{Anthropic, Gemini, OpenAi};
    let estimate = |r: Rate, per_minute: f64| Rate {
        per_minute_estimate: Some(per_minute),
        ..r
    };
    // Gemini bills 32 tokens per second of audio, so a minute is 1 920 input tokens.
    let gemini_minute = |input: f64| 1_920.0 * input / 1_000_000.0;
    let gemini_note = Some(
        "Check this rate: the pricing page lists no separate audio price for this model".into(),
    );
    vec![
        Rate::minute(OpenAi, "whisper-1", 0.006, OPENAI_PRICING),
        Rate::minute(OpenAi, "gpt-transcribe", 0.0045, OPENAI_PRICING),
        estimate(
            Rate::tokens(OpenAi, "gpt-4o-transcribe", 2.50, 10.00, OPENAI_PRICING),
            0.006,
        ),
        estimate(
            Rate::tokens(OpenAi, "gpt-4o-mini-transcribe", 1.25, 5.00, OPENAI_PRICING),
            0.003,
        ),
        Rate {
            cached_input: Some(0.075),
            ..Rate::tokens(OpenAi, "gpt-4o-mini", 0.15, 0.60, OPENAI_PRICING)
        },
        Rate {
            cached_input: Some(0.125),
            ..Rate::tokens(OpenAi, "gpt-5", 1.25, 10.00, GPT5_PRICING)
        },
        Rate {
            cached_input: Some(0.025),
            ..Rate::tokens(OpenAi, "gpt-5-mini", 0.25, 2.00, GPT5_MINI_PRICING)
        },
        Rate {
            note: gemini_note.clone(),
            ..estimate(
                Rate::tokens(Gemini, "gemini-3.8-flash", 0.75, 3.75, GEMINI_PRICING),
                gemini_minute(0.75),
            )
        },
        Rate {
            valid_from: Some("2027-01-01".into()),
            note: gemini_note,
            ..estimate(
                Rate::tokens(Gemini, "gemini-3.8-flash", 1.50, 7.50, GEMINI_PRICING),
                gemini_minute(1.50),
            )
        },
        Rate {
            cached_input: Some(0.20),
            cache_write: Some(5.00),
            ..Rate::tokens(Anthropic, "claude-opus-5-5", 4.00, 20.00, ANTHROPIC_PRICING)
        },
    ]
}

/// The rates in force: the defaults with the user's entries over them.
#[derive(Clone, Debug, PartialEq)]
pub struct RateTable {
    pub entries: Vec<(Rate, RateOrigin)>,
    /// One line per user entry that was ignored (F16-FR8).
    pub problems: Vec<String>,
}

impl Default for RateTable {
    fn default() -> Self {
        RateTable::effective(&[])
    }
}

/// A user entry as stored, or the line Settings shows for it.
pub fn parse_user(v: &Value) -> Result<Rate, String> {
    let named = v["model"]
        .as_str()
        .filter(|m| m.len() <= MAX_MODEL_LEN && !m.chars().any(char::is_control))
        .map_or_else(|| "a model".to_owned(), str::to_owned);
    let bad = || format!("Your rate for {named} has an error; using the default");
    let r: Rate = serde_json::from_value(v.clone()).map_err(|_| bad())?;
    r.check().map_err(|_| bad())?;
    Ok(r)
}

impl RateTable {
    /// The defaults with `user` (settings.json's `rates`) over them. A bad entry is skipped
    /// and named in `problems` (F16-FR8); a later entry with the same key wins.
    pub fn effective(user: &[Value]) -> RateTable {
        let mut entries: Vec<(Rate, RateOrigin)> = defaults()
            .into_iter()
            .map(|r| (r, RateOrigin::Default))
            .collect();
        let mut problems = Vec::new();
        for v in user {
            match parse_user(v) {
                Ok(r) => {
                    entries.retain(|(e, _)| e.key() != r.key());
                    entries.push((r, RateOrigin::User));
                }
                Err(p) => problems.push(p),
            }
        }
        entries.sort_by(|(a, _), (b, _)| a.key().cmp(&b.key()));
        RateTable { entries, problems }
    }

    /// The newest `asOf` among the entries in force ("rates as of …").
    pub fn as_of(&self) -> String {
        self.entries
            .iter()
            .map(|(r, _)| r.as_of.as_str())
            .max()
            .unwrap_or(DEFAULTS_AS_OF)
            .to_owned()
    }

    /// The default entry with the same key as `r`, if any.
    pub fn default_for(r: &Rate) -> Option<Rate> {
        defaults().into_iter().find(|d| d.key() == r.key())
    }

    /// The entry pricing `model` on `provider` on `day`: the exact model id if listed, else the
    /// id the provider returned without its `models/` prefix or dated-snapshot suffix
    /// (`gpt-5-2025-08-07` → `gpt-5`, `claude-…-20250929`), else the longest listed prefix
    /// ending at a `-`; of its entries, the latest `validFrom` on or before `day`.
    pub fn lookup(&self, provider: Provider, model: &str, day: NaiveDate) -> Option<&Rate> {
        let ours = || {
            self.entries
                .iter()
                .map(|(r, _)| r)
                .filter(|r| r.provider == provider)
        };
        let base = base_model(model);
        let at_boundary = |m: &str| {
            [model, base]
                .iter()
                .any(|id| id.strip_prefix(m).is_some_and(|rest| rest.starts_with('-')))
        };
        let name = ours()
            .map(|r| r.model.as_str())
            .find(|m| model == *m)
            .or_else(|| ours().map(|r| r.model.as_str()).find(|m| base == *m))
            .or_else(|| {
                ours()
                    .map(|r| r.model.as_str())
                    .filter(|m| at_boundary(m))
                    .max_by_key(|m| m.len())
            })?;
        ours()
            .filter(|r| r.model == name)
            .filter(|r| {
                r.valid_from
                    .as_deref()
                    .and_then(date)
                    .is_none_or(|d| d <= day)
            })
            .max_by_key(|r| r.valid_from.as_deref().and_then(date))
    }
}

/// A provider-returned model id as its base id: without Gemini's `models/` prefix and without
/// a dated-snapshot suffix (`-2025-08-07`, `-20250929`, `@20250929`).
pub fn base_model(id: &str) -> &str {
    let id = id.strip_prefix("models/").unwrap_or(id);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let dashed_date = |s: &str| {
        let p: Vec<&str> = s.split('-').collect();
        p.len() == 3
            && digits(p[0])
            && p[0].len() == 4
            && p[1].len() == 2
            && p[2].len() == 2
            && digits(p[1])
            && digits(p[2])
    };
    if id.len() > 11 {
        let (head, tail) = id.split_at(id.len() - 11);
        if tail.starts_with('-') && dashed_date(&tail[1..]) {
            return head;
        }
    }
    if let Some(i) = id.rfind(['-', '@']) {
        let tail = &id[i + 1..];
        if tail.len() == 8 && digits(tail) && i > 0 {
            return &id[..i];
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(s: &str) -> NaiveDate {
        date(s).unwrap()
    }

    #[test]
    fn every_default_is_valid_and_dated() {
        for r in defaults() {
            assert_eq!(r.check(), Ok(()), "{r:?}");
            assert_eq!(r.as_of, DEFAULTS_AS_OF);
            assert!(r.source.starts_with("https://"), "{r:?}");
        }
        assert_eq!(RateTable::default().as_of(), DEFAULTS_AS_OF);
        assert!(RateTable::default().problems.is_empty());
    }

    #[test]
    fn exact_ids_win_then_the_longest_prefix() {
        let t = RateTable::default();
        let d = day("2026-10-09");
        let m = |p, id| t.lookup(p, id, d).map(|r| r.model.as_str());
        assert_eq!(m(Provider::OpenAi, "gpt-4o-mini"), Some("gpt-4o-mini"));
        assert_eq!(
            m(Provider::OpenAi, "gpt-4o-mini-2024-07-18"),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            m(Provider::OpenAi, "gpt-4o-mini-transcribe"),
            Some("gpt-4o-mini-transcribe")
        );
        assert_eq!(
            m(Provider::OpenAi, "gpt-4o-mini-transcribe-2025-03"),
            Some("gpt-4o-mini-transcribe")
        );
        assert_eq!(m(Provider::OpenAi, "gpt-5"), Some("gpt-5"));
        assert_eq!(
            m(Provider::OpenAi, "gpt-4o"),
            None,
            "no rate for it, and no prefix of it"
        );
        assert_eq!(m(Provider::Gemini, "whisper-1"), None, "per provider");
    }

    /// Device finding 2026-10-09: OpenAI reported `gpt-5-2025-08-07` and the record went
    /// unpriced. Dated snapshots and prefixed ids price as their base model.
    #[test]
    fn provider_returned_variants_price_as_their_base_model() {
        let t = RateTable::default();
        let d = day("2026-10-09");
        let m = |p, id| t.lookup(p, id, d).map(|r| r.model.as_str());
        assert_eq!(m(Provider::OpenAi, "gpt-5-2025-08-07"), Some("gpt-5"));
        assert_eq!(
            m(Provider::OpenAi, "gpt-4o-mini-2024-07-18"),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            m(Provider::Anthropic, "claude-opus-5-5-20250929"),
            Some("claude-opus-5-5")
        );
        assert_eq!(
            m(Provider::Anthropic, "claude-opus-5-5@20250929"),
            Some("claude-opus-5-5")
        );
        assert_eq!(
            m(Provider::Gemini, "models/gemini-3.8-flash"),
            Some("gemini-3.8-flash")
        );
        assert_eq!(
            m(Provider::Gemini, "models/gemini-3.8-flash-001"),
            Some("gemini-3.8-flash")
        );
        // A prefix counts only at a `-`: `gpt-5` prices `gpt-5-chat`, never `gpt-50`.
        assert_eq!(m(Provider::OpenAi, "gpt-5-chat"), Some("gpt-5"));
        assert_eq!(
            m(Provider::OpenAi, "gpt-5-mini-2025-08-07"),
            Some("gpt-5-mini")
        );
        assert_eq!(m(Provider::OpenAi, "gpt-50"), None);
        assert_eq!(base_model("gpt-5-2025-08-07"), "gpt-5");
        assert_eq!(base_model("x-2025-8-07"), "x-2025-8-07");
        assert_eq!(base_model("x-abcd-08-07"), "x-abcd-08-07");
        assert_eq!(base_model("model-1234567"), "model-1234567");
        assert_eq!(base_model("-20250929"), "-20250929");
        assert_eq!(base_model("whisper-1"), "whisper-1");
        assert_eq!(base_model("models/x"), "x");
    }

    #[test]
    fn valid_from_picks_the_price_in_force() {
        let t = RateTable::default();
        let input = |d| {
            t.lookup(Provider::Gemini, "gemini-3.8-flash", day(d))
                .unwrap()
                .input
        };
        assert_eq!(input("2026-12-31"), Some(0.75));
        assert_eq!(input("2027-01-01"), Some(1.50));
        assert_eq!(input("2030-06-01"), Some(1.50));
    }

    #[test]
    fn a_user_entry_replaces_its_default_and_a_bad_one_is_named() {
        let user = [
            json!({"provider": "openai", "model": "whisper-1", "unit": "minute",
                   "perMinute": 0.01, "asOf": "2026-11-02", "source": USER_SOURCE}),
            json!({"provider": "openai", "model": "gpt-x", "unit": "tokens", "input": 1,
                   "asOf": "2026-11-02", "source": USER_SOURCE}),
            json!({"provider": "openai", "model": "gpt-y", "unit": "minute", "perMinute": -1,
                   "asOf": "2026-11-02", "source": USER_SOURCE}),
            json!(7),
        ];
        let t = RateTable::effective(&user);
        let w = t
            .lookup(Provider::OpenAi, "whisper-1", day("2026-11-03"))
            .unwrap();
        assert_eq!(w.per_minute, Some(0.01));
        let n = t
            .entries
            .iter()
            .filter(|(r, _)| r.model == "whisper-1")
            .count();
        assert_eq!(n, 1, "replaced, not added");
        assert_eq!(t.as_of(), "2026-11-02");
        assert_eq!(
            t.problems,
            [
                "Your rate for gpt-x has an error; using the default",
                "Your rate for gpt-y has an error; using the default",
                "Your rate for a model has an error; using the default",
            ]
        );
        assert!(
            t.lookup(Provider::OpenAi, "gpt-x", day("2026-11-03"))
                .is_none()
        );
    }

    #[test]
    fn checks_name_the_field() {
        let ok = Rate::tokens(Provider::OpenAi, "m", 1.0, 2.0, "s");
        let bad = |r: Rate| r.check().unwrap_err().0;
        assert_eq!(
            bad(Rate {
                model: "a b".into(),
                ..ok.clone()
            }),
            "model"
        );
        assert_eq!(
            bad(Rate {
                output: None,
                ..ok.clone()
            }),
            "output"
        );
        assert_eq!(
            bad(Rate {
                input: None,
                ..ok.clone()
            }),
            "input"
        );
        assert_eq!(
            bad(Rate {
                cached_input: Some(f64::NAN),
                ..ok.clone()
            }),
            "cachedInput"
        );
        let minute = Rate::base(Provider::OpenAi, "m", Unit::Minute, "s");
        assert_eq!(bad(minute), "perMinute");
        assert_eq!(
            bad(Rate {
                valid_from: Some("soon".into()),
                ..ok.clone()
            }),
            "validFrom"
        );
        assert_eq!(
            bad(Rate {
                as_of: "x".into(),
                ..ok.clone()
            }),
            "asOf"
        );
        assert_eq!(
            bad(Rate {
                note: Some("\n".into()),
                ..ok.clone()
            }),
            "source"
        );
        assert!(ok.same_prices(&ok.clone()));
        assert!(!ok.same_prices(&Rate {
            output: Some(3.0),
            ..ok.clone()
        }));
        assert_eq!(
            RateTable::default_for(&defaults()[0]).map(|r| r.model),
            Some("whisper-1".into())
        );
    }
}
