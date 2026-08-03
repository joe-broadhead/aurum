//! First-party OpenAI TTS via `/audio/speech` (JOE-1940).
//!
//! Reuses [`OpenAiSpeechRequest`] protocol; auth/origin via [`OpenAiHttpPolicy`].

use crate::audio::{normalize_remote_audio, BoundedAudioBody, RemoteAudioLimits};
use crate::error::{ProviderError, Result, UserError};
use crate::remote::{
    map_http_status, read_body_limited_with_op, resolve_encoded_format, send_with_op,
    ExpectedWireFormat, HardenedHttpClient, OpenAiHttpPolicy, OpenAiSpeechRequest,
    RemoteBodyLimits, RemotePolicy, SpeechResponseFormat,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use crate::tts::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use crate::tts::validate::{clamp_speaking_rate, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
use async_trait::async_trait;
use std::sync::Arc;

const PROVIDER_NAME: &str = "openai";

/// Default reviewed OpenAI TTS model.
pub const DEFAULT_OPENAI_TTS_MODEL: &str = "tts-1";

/// Default OpenAI voice.
pub const DEFAULT_OPENAI_TTS_VOICE: &str = "alloy";

/// OpenAI TTS catalogue entry.
#[derive(Debug, Clone, Copy)]
pub struct OpenAiTtsRecord {
    pub model: &'static str,
    pub voices: &'static [&'static str],
    pub default_sample_rate_hz: u32,
    pub max_text_chars: usize,
    pub rate_min: f32,
    pub rate_max: f32,
}

/// Static reviewed OpenAI TTS models (JOE-1940).
pub static OPENAI_TTS_REGISTRY: &[OpenAiTtsRecord] = &[
    OpenAiTtsRecord {
        model: "tts-1",
        voices: &[
            "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
            "verse",
        ],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
    },
    OpenAiTtsRecord {
        model: "tts-1-hd",
        voices: &[
            "alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
            "verse",
        ],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
    },
    OpenAiTtsRecord {
        model: "gpt-4o-mini-tts",
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

pub fn lookup_openai_tts(model: &str) -> Option<&'static OpenAiTtsRecord> {
    let m = model.trim();
    OPENAI_TTS_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// First-party OpenAI speech provider.
pub struct OpenAiTtsProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for OpenAiTtsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiTtsProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .finish()
    }
}

impl OpenAiTtsProvider {
    pub fn with_policy(
        api_key: Option<SecretString>,
        base_url: Option<String>,
        mut policy: RemotePolicy,
    ) -> Result<Self> {
        let api_key = api_key
            .filter(|s| !s.expose().trim().is_empty())
            .ok_or(UserError::MissingApiKey)?;

        if base_url
            .as_deref()
            .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"))
        {
            policy.allow_loopback_http = true;
        }

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, OpenAiHttpPolicy)?;
        Ok(Self {
            api_key,
            http,
            governor: ResourceGovernor::process_global(),
        })
    }

    /// Bind an engine-local governor (preferred for long-lived hosts).
    pub fn with_governor(mut self, governor: Arc<ResourceGovernor>) -> Self {
        self.governor = governor;
        self
    }

    fn resolve_model_voice(model: &str, voice: &str) -> Result<(&'static OpenAiTtsRecord, String)> {
        let rec = lookup_openai_tts(model).ok_or_else(|| UserError::UnsupportedCapability {
            provider: PROVIDER_NAME.into(),
            model: model.into(),
            reason: "model is not in the reviewed OpenAI TTS registry".into(),
            hint: format!(
                "use one of: {}",
                OPENAI_TTS_REGISTRY
                    .iter()
                    .map(|r| r.model)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })?;
        let voice = voice.trim();
        if voice.is_empty() {
            return Ok((rec, DEFAULT_OPENAI_TTS_VOICE.into()));
        }
        let ok = rec.voices.iter().any(|v| v.eq_ignore_ascii_case(voice));
        if !ok {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: model.into(),
                reason: format!("voice '{voice}' is not supported for this OpenAI TTS model"),
                hint: format!("supported voices: {}", rec.voices.join(", ")),
            }
            .into());
        }
        Ok((rec, voice.to_ascii_lowercase()))
    }
}

#[async_trait]
impl SynthesisProvider for OpenAiTtsProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult> {
        if opts.pack_dir.is_some() || opts.allow_unverified {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: opts.model.clone(),
                reason: "local pack_dir/allow_unverified are not valid for remote TTS".into(),
                hint: "omit pack_dir; use a local TTS provider for custom packs".into(),
            }
            .into());
        }

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
                    "TTS text exceeds OpenAI model limit ({} chars)",
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
        let op = opts.resolve_op_context();
        op.check()?;
        op.emit("tts", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
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

        let response = send_with_op(
            self.http
                .request(reqwest::Method::POST, "audio/speech", self.api_key.expose())?
                .header("Content-Type", "application/json")
                .body(json),
            &op,
            PROVIDER_NAME,
        )
        .await?;

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
        let bytes = read_body_limited_with_op(
            response,
            PROVIDER_NAME,
            RemoteBodyLimits {
                max_bytes: body_cap,
            },
            &op,
        )
        .await?;
        op.check()?;
        map_http_status(PROVIDER_NAME, status, "")?;

        // OpenAI may return audio/mpeg even when pcm was requested on older models —
        // prefer PCM path when content-type or default rate applies.
        // Requested PCM; accept only allowlisted PCM MIME (JOE-1977). MP3 only if MIME is mp3.
        let expected = ExpectedWireFormat::pcm(rec.default_sample_rate_hz, 1);
        let format = if content_type.to_ascii_lowercase().contains("mpeg")
            || content_type.to_ascii_lowercase().contains("audio/mp3")
        {
            // Fail closed: we requested PCM; MP3 response is a contract violation unless
            // explicitly allowed — reject (OpenAI docs say pcm when requested).
            return Err(ProviderError::InvalidProviderPayload {
                provider: PROVIDER_NAME.into(),
                reason: "expected PCM but Content-Type indicates MP3".into(),
            }
            .into());
        } else {
            resolve_encoded_format(PROVIDER_NAME, expected, &content_type, &bytes)?
        };

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
        assert!(lookup_openai_tts("tts-1").is_some());
        assert!(lookup_openai_tts("nope").is_none());
    }

    #[test]
    fn rejects_local_voice_alias() {
        let err = OpenAiTtsProvider::resolve_model_voice("tts-1", "Luna").unwrap_err();
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
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "audio/pcm;rate=24000;channels=1")
                    .set_body_bytes(pcm),
            )
            .mount(&server)
            .await;

        let provider = OpenAiTtsProvider::with_policy(
            Some("sk-test".into()),
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
                "Hello OpenAI",
                &SynthesisOptions {
                    model: "tts-1".into(),
                    voice: "alloy".into(),
                    language: "en".into(),
                    sample_rate_hz: None,
                    speaking_rate: 1.0,
                    timeout_ms: 30_000,
                    cancel: None,
                    op: None,
                    local_only: false,
                    pack_dir: None,
                    allow_unverified: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(result.provider, "openai");
        assert_eq!(result.backend_kind, BackendKind::Remote);
        assert_eq!(result.sample_rate_hz, 24_000);
        assert_eq!(result.pcm_i16_mono.len(), n);
    }
}
