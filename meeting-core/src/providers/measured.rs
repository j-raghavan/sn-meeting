//! F16-FR1: what a provider call reported it used, in Meetings' own shape (no rig type leaves the
//! providers module, R8). Counters are copied from rig's normalised usage; the fields rig does not
//! normalise (OpenAI transcription's `usage.type`, `usage.seconds` and the audio/text split,
//! Gemini's `promptTokensDetails`) are read from the reply document.
use crate::keyfile::Provider;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One call's measured usage. `None` means the provider did not report that counter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub provider: Provider,
    /// The model the reply named, else the one requested.
    pub model: String,
    /// Transcription: the audio's measured length, when known.
    pub audio_ms: Option<u64>,
    /// Every input token: uncached, cached and cache writes (rig's `input_tokens`).
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    /// OpenAI transcription's `usage.type`: `duration` or `tokens`.
    pub usage_type: Option<String>,
    /// OpenAI transcription's `usage.seconds` (duration-billed models).
    pub seconds: Option<f64>,
    /// The audio part of the input, from OpenAI's `input_token_details` or Gemini's
    /// `promptTokensDetails` (modality `AUDIO`).
    pub audio_input_tokens: Option<u64>,
    /// The text part of the input, from the same fields (`text_tokens`, modality `TEXT`).
    pub text_input_tokens: Option<u64>,
}

impl Measured {
    /// Nothing reported yet for `model` on `provider`.
    pub fn new(provider: Provider, model: &str) -> Measured {
        Measured {
            provider,
            model: model.to_owned(),
            audio_ms: None,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            usage_type: None,
            seconds: None,
            audio_input_tokens: None,
            text_input_tokens: None,
        }
    }

    /// Whether any token count was reported.
    pub fn has_tokens(&self) -> bool {
        self.input_tokens.is_some() || self.output_tokens.is_some()
    }

    /// Fill the fields rig leaves in the reply document. A field that is absent or of the
    /// wrong type is left as it is: a reply is untrusted input.
    pub fn read_raw(&mut self, raw: &Value) {
        let usage = &raw["usage"];
        if let Some(t) = usage["type"].as_str().filter(|t| t.len() <= 32) {
            self.usage_type = Some(t.to_owned());
        }
        if let Some(s) = usage["seconds"]
            .as_f64()
            .filter(|s| s.is_finite() && *s >= 0.0)
        {
            self.seconds = Some(s);
        }
        let details = &usage["input_token_details"];
        if let Some(a) = details["audio_tokens"].as_u64() {
            self.audio_input_tokens = Some(a);
        }
        if let Some(t) = details["text_tokens"].as_u64() {
            self.text_input_tokens = Some(t);
        }
        // Gemini: `usageMetadata.promptTokensDetails: [{modality, tokenCount}]`.
        if let Some(parts) = raw["usageMetadata"]["promptTokensDetails"].as_array() {
            let count = |m: &str| {
                let mut found = false;
                let n = parts
                    .iter()
                    .filter(|p| p["modality"].as_str() == Some(m))
                    .filter_map(|p| p["tokenCount"].as_u64())
                    .inspect(|_| found = true)
                    .fold(0u64, u64::saturating_add);
                found.then_some(n)
            };
            if let Some(a) = count("AUDIO") {
                self.audio_input_tokens = Some(a);
            }
            if let Some(t) = count("TEXT") {
                self.text_input_tokens = Some(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_duration_and_token_details_are_read() {
        let mut m = Measured::new(Provider::OpenAi, "whisper-1");
        m.read_raw(&json!({"usage": {"type": "duration", "seconds": 30}}));
        assert_eq!(m.usage_type.as_deref(), Some("duration"));
        assert_eq!(m.seconds, Some(30.0));
        assert!(!m.has_tokens());
        let mut m = Measured::new(Provider::OpenAi, "gpt-4o-transcribe");
        m.read_raw(&json!({"usage": {"type": "tokens", "input_tokens": 500,
            "input_token_details": {"audio_tokens": 480, "text_tokens": 20}}}));
        assert_eq!(
            (m.audio_input_tokens, m.text_input_tokens),
            (Some(480), Some(20))
        );
    }

    #[test]
    fn gemini_modalities_are_summed_by_kind() {
        let mut m = Measured::new(Provider::Gemini, "gemini-3.8-flash");
        m.read_raw(&json!({"usageMetadata": {"promptTokensDetails": [
            {"modality": "AUDIO", "tokenCount": 960}, {"modality": "TEXT", "tokenCount": 80},
            {"modality": "TEXT", "tokenCount": 2}, {"modality": "IMAGE"}]}}));
        assert_eq!(
            (m.audio_input_tokens, m.text_input_tokens),
            (Some(960), Some(82))
        );
        let mut m = Measured::new(Provider::Gemini, "g");
        m.read_raw(&json!({"usageMetadata": {"promptTokensDetails": [
            {"modality": "TEXT", "tokenCount": 3}]}}));
        assert_eq!((m.audio_input_tokens, m.text_input_tokens), (None, Some(3)));
    }

    #[test]
    fn junk_is_ignored() {
        let mut m = Measured::new(Provider::OpenAi, "m");
        m.read_raw(
            &json!({"usage": {"type": 3, "seconds": -1, "input_token_details": "x"},
            "usageMetadata": {"promptTokensDetails": {}}}),
        );
        m.read_raw(&Value::Null);
        assert_eq!(m, Measured::new(Provider::OpenAi, "m"));
    }
}
