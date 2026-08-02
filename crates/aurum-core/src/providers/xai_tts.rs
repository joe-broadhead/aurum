//! xAI REST TTS via `/audio/speech` (JOE-1942).
//!
//! Reuses [`OpenAiSpeechRequest`] protocol; auth/origin via [`XaiHttpPolicy`].

use crate::audio::{
    normalize_remote_audio, BoundedAudioBody, EncodedAudioFormat, RemoteAudioLimits,
};
use crate::error::{ProviderError, Result, UserError};
use crate::remote::{
    map_http_status, parse_pcm_content_type, read_body_limited, HardenedHttpClient,
    OpenAiSpeechRequest, RemoteBodyLimits, RemotePolicy, SpeechResponseFormat, XaiHttpPolicy,
};
use crate::runtime::OpContext;
use crate::tts::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use crate::tts::validate::{clamp_speaking_rate, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
use async_trait::async_trait;
use std::time::Duration;

const PROVIDER_NAME: &str = "xai";

/// Default reviewed xAI TTS model.
pub const DEFAULT_XAI_TTS_MODEL: &str = "grok-tts";

/// Default xAI voice.
pub const DEFAULT_XAI_TTS_VOICE: &str = "alloy";

/// xAI TTS catalogue entry.
#[derive(Debug, Clone, Copy)]
pub struct XaiTtsRecord {
    pub model: &'static str,
    pub voices: &'static [&'static str],
    pub default_sample_rate_hz: u32,
    pub max_text_chars: usize,
    pub rate_min: f32,
    pub rate_max: f32,
}

/// Static reviewed xAI TTS models (JOE-1942).
pub static XAI_TTS_REGISTRY: &[XaiTtsRecord] = &[
    XaiTtsRecord {
        model: "grok-tts",
        voices: &[
            "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
            "verse",
        ],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
    },
    XaiTtsRecord {
        model: "grok-tts-hd",
        voices: &[
            "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
            "verse",
        ],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
    },
    XaiTtsRecord {
        model: "grok-tts-mini",
        voices: &[
            "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
            "verse",
        ],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
    },
];

pub fn lookup_xai_tts(model: &str) -> Option<&'static XaiTtsRecord> {
    let m = model.trim();
    XAI_TTS_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// First-party xAI speech provider.
pub struct XaiTtsProvider {
    api_key: String,
    http: HardenedHttpClient,
}

impl std::fmt::Debug for XaiTtsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiTtsProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .finish()
    }
}

impl XaiTtsProvider {
    pub fn with_policy(
        api_key: Option<String>,
        base_url: Option<String>,
        mut policy: RemotePolicy,
    ) -> Result<Self> {
        let api_key = api_key
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(UserError::MissingApiKey)?;

        if base_url
            .as_deref()
            .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"))
        {
            policy.allow_loopback_http = true;
        }

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, XaiHttpPolicy)?;
        Ok(Self { api_key, http })
    }

    fn resolve_model_voice(model: &str, voice: &str) -> Result<(&'static XaiTtsRecord, String)> {
        let rec = lookup_xai_tts(model).ok_or_else(|| UserError::UnsupportedCapability {
            provider: PROVIDER_NAME.into(),
            model: model.into(),
            reason: "model is not in the reviewed xAI TTS registry".into(),
            hint: format!(
                "use one of: {}",
                XAI_TTS_REGISTRY
                    .iter()
                    .map(|r| r.model)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })?;
        let voice = voice.trim();
        if voice.is_empty() {
            return Ok((rec, DEFAULT_XAI_TTS_VOICE.into()));
        }
        let ok = rec.voices.iter().any(|v| v.eq_ignore_ascii_case(voice));
        if !ok {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: model.into(),
                reason: format!("voice '{voice}' is not supported for this xAI TTS model"),
                hint: format!("supported voices: {}", rec.voices.join(", ")),
            }
            .into());
        }
        Ok((rec, voice.to_ascii_lowercase()))
    }
}

#[async_trait]
impl SynthesisProvider for XaiTtsProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult> {
        if opts.local_only {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: opts.model.clone(),
                reason: "remote TTS is disabled under local_only".into(),
                hint: "unset local_only or use provider=local".into(),
            }
            .into());
        }

        let text = text.trim();
        if text.is_empty() {
            return Err(UserError::Other {
                message: "TTS text is empty".into(),
            }
            .into());
        }

        let (rec, voice) = Self::resolve_model_voice(&opts.model, &opts.voice)?;
        if text.chars().count() > rec.max_text_chars {
            return Err(UserError::Other {
                message: format!(
                    "TTS text exceeds xAI model limit ({} chars)",
                    rec.max_text_chars
                ),
            }
            .into());
        }

        if !opts.speaking_rate.is_finite()
            || opts.speaking_rate < SPEAKING_RATE_MIN
            || opts.speaking_rate > SPEAKING_RATE_MAX
        {
            return Err(UserError::Other {
                message: format!(
                    "speaking rate must be finite in {SPEAKING_RATE_MIN}..={SPEAKING_RATE_MAX}"
                ),
            }
            .into());
        }
        let rate = clamp_speaking_rate(opts.speaking_rate);
        if rate < rec.rate_min || rate > rec.rate_max {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: opts.model.clone(),
                reason: format!(
                    "speaking rate {rate} outside model range {}..={}",
                    rec.rate_min, rec.rate_max
                ),
                hint: format!("use a rate in {}..={}", rec.rate_min, rec.rate_max),
            }
            .into());
        }

        let timeout = Duration::from_millis(opts.timeout_ms.max(1));
        let op =
            OpContext::from_optional_cancel(opts.cancel.clone()).with_deadline_from_now(timeout);
        op.check()?;
        op.emit("tts", "request");

        // Prefer PCM for direct normalize (JOE-1937).
        let body = OpenAiSpeechRequest::new(
            &opts.model,
            text,
            &voice,
            SpeechResponseFormat::Pcm,
            Some(rate),
        );
        let json = body.to_json_bytes().map_err(|e| ProviderError::Other {
            message: format!("speech request serialize: {e}"),
        })?;

        let response = self
            .http
            .request(reqwest::Method::POST, "audio/speech", &self.api_key)?
            .header("Content-Type", "application/json")
            .body(json)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                provider: PROVIDER_NAME.into(),
                reason: e.to_string(),
            })?;

        op.check()?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        op.emit("tts", "read_body");
        let body_cap = if status.is_success() {
            RemoteAudioLimits::default().max_encoded_bytes
        } else {
            64 * 1024
        };
        let bytes = read_body_limited(
            response,
            PROVIDER_NAME,
            RemoteBodyLimits {
                max_bytes: body_cap,
            },
        )
        .await?;
        op.check()?;
        map_http_status(PROVIDER_NAME, status, "")?;

        // xAI may return audio/mpeg even when pcm was requested on older models —
        // prefer PCM path when content-type or default rate applies.
        let (format, sample_hint) = if content_type.contains("mpeg") || content_type.contains("mp3")
        {
            (EncodedAudioFormat::Mp3, rec.default_sample_rate_hz)
        } else {
            let (rate_hz, ch) =
                parse_pcm_content_type(&content_type).unwrap_or((rec.default_sample_rate_hz, 1));
            (
                EncodedAudioFormat::PcmS16Le {
                    sample_rate_hz: rate_hz,
                    channels: ch,
                },
                rate_hz,
            )
        };
        let _ = sample_hint;

        let bounded = BoundedAudioBody::try_from_bytes(
            bytes,
            RemoteAudioLimits::default().max_encoded_bytes,
            PROVIDER_NAME,
        )?;
        op.emit("tts", "normalize");
        let norm = normalize_remote_audio(
            bounded,
            format,
            RemoteAudioLimits::default(),
            &op,
            PROVIDER_NAME,
        )
        .await?;

        Ok(SynthesisResult {
            pcm_i16_mono: norm.pcm_i16_mono,
            sample_rate_hz: norm.sample_rate_hz,
            channels: 1,
            backend_kind: BackendKind::Remote,
            provider: PROVIDER_NAME.into(),
            model: opts.model.clone(),
            voice,
            language: opts.language.clone(),
            duration_ms: norm.duration_ms,
            text_chars: text.chars().count(),
            text_truncated: false,
            chunk_count: 1,
            synthesized_chars: text.chars().count(),
            adapter: None,
            trust: None,
            provenance: None,
        })
    }

    async fn preload(&self, _model: &str, _voice: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn registry_lookup() {
        assert!(lookup_xai_tts("grok-tts").is_some());
        assert!(lookup_xai_tts("nope").is_none());
    }

    #[test]
    fn rejects_local_voice_alias() {
        let err = XaiTtsProvider::resolve_model_voice("grok-tts", "Luna").unwrap_err();
        assert!(err.to_string().contains("voice") || err.to_string().contains("Luna"));
    }

    #[tokio::test]
    async fn mock_pcm_speech() {
        let server = MockServer::start().await;
        let n = 2_400;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = ((i % 40) as i16) * 10;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        Mock::given(method("POST"))
            .and(path("/audio/speech"))
            .and(header("Authorization", "Bearer xai-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "audio/pcm;rate=24000;channels=1")
                    .set_body_bytes(pcm),
            )
            .mount(&server)
            .await;

        let provider = XaiTtsProvider::with_policy(
            Some("xai-test".into()),
            Some(server.uri()),
            RemotePolicy {
                allow_loopback_http: true,
                allow_custom_credentialed_endpoint: true,
                ..Default::default()
            },
        )
        .unwrap();

        let result = provider
            .synthesize(
                "Hello xAI",
                &SynthesisOptions {
                    model: "grok-tts".into(),
                    voice: "alloy".into(),
                    language: "en".into(),
                    sample_rate_hz: None,
                    speaking_rate: 1.0,
                    timeout_ms: 30_000,
                    cancel: None,
                    local_only: false,
                    pack_dir: None,
                    allow_unverified: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(result.provider, "xai");
        assert_eq!(result.backend_kind, BackendKind::Remote);
        assert_eq!(result.sample_rate_hz, 24_000);
        assert_eq!(result.pcm_i16_mono.len(), n);
    }
}
