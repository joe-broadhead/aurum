//! OpenAI-compatible audio speech request protocol (JOE-1939).
//!
//! Encodes documented speech request fields without credentials or endpoint
//! logic. Shared by OpenRouter TTS and future OpenAI first-party TTS.

use serde::{Deserialize, Serialize};

/// Wire `response_format` values for `/audio/speech`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechResponseFormat {
    /// Raw s16le PCM (preferred for Aurum normalization).
    Pcm,
    /// MPEG Layer III (normalized via supervised FFmpeg).
    Mp3,
}

impl SpeechResponseFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pcm => "pcm",
            Self::Mp3 => "mp3",
        }
    }
}

/// OpenAI-compatible speech request body (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiSpeechRequest {
    pub model: String,
    pub input: String,
    pub voice: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<SpeechResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
}

impl OpenAiSpeechRequest {
    /// Build a request, omitting unsupported/default fields when helpful.
    pub fn new(
        model: impl Into<String>,
        input: impl Into<String>,
        voice: impl Into<String>,
        format: SpeechResponseFormat,
        speed: Option<f32>,
    ) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            voice: voice.into(),
            response_format: Some(format),
            speed: speed.map(|s| s as f64),
        }
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

/// Parse sample rate from `Content-Type` like `audio/pcm;rate=24000;channels=1`.
///
/// Returns `(sample_rate_hz, channels)` when both present; defaults channels to 1
/// when only rate is present.
pub fn parse_pcm_content_type(content_type: &str) -> Option<(u32, u16)> {
    let ct = content_type.to_ascii_lowercase();
    if !ct.contains("audio/pcm") && !ct.starts_with("audio/l16") {
        return None;
    }
    let mut rate = None;
    let mut channels = None;
    for part in ct.split(';').skip(1) {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("rate=") {
            rate = v.trim().parse().ok();
        } else if let Some(v) = part.strip_prefix("channels=") {
            channels = v.trim().parse().ok();
        }
    }
    let rate = rate?;
    Some((rate, channels.unwrap_or(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_speech_request() {
        let req = OpenAiSpeechRequest::new(
            "openai/gpt-4o-mini-tts",
            "Hello",
            "alloy",
            SpeechResponseFormat::Pcm,
            Some(1.0),
        );
        let v: serde_json::Value = serde_json::from_slice(&req.to_json_bytes().unwrap()).unwrap();
        assert_eq!(v["model"], "openai/gpt-4o-mini-tts");
        assert_eq!(v["input"], "Hello");
        assert_eq!(v["voice"], "alloy");
        assert_eq!(v["response_format"], "pcm");
        assert_eq!(v["speed"], 1.0);
        // No secrets ever.
        let s = v.to_string();
        assert!(!s.contains("Authorization"));
        assert!(!s.contains("api_key"));
    }

    #[test]
    fn omits_speed_when_none() {
        let req = OpenAiSpeechRequest::new("m", "i", "v", SpeechResponseFormat::Mp3, None);
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("speed"));
        assert!(s.contains("mp3"));
    }

    #[test]
    fn parse_pcm_content_type_rate_channels() {
        assert_eq!(
            parse_pcm_content_type("audio/pcm;rate=24000;channels=1"),
            Some((24_000, 1))
        );
        assert_eq!(
            parse_pcm_content_type("AUDIO/PCM; rate=16000"),
            Some((16_000, 1))
        );
        assert!(parse_pcm_content_type("audio/mpeg").is_none());
    }
}
