//! xAI REST TTS via official `POST /v1/tts` (JOE-1942 / JOE-1976).
//!
//! Contract: https://docs.x.ai/developers/rest-api-reference/inference/voice
//! Request: text, voice_id, language, output_format {codec, sample_rate}, speed.
//! Built-in voices: eve, ara, leo, rex, sal. No OpenAI voices or `/audio/speech`.

use crate::audio::{normalize_remote_audio, BoundedAudioBody, RemoteAudioLimits};
use crate::error::{ProviderError, Result, UserError};
use crate::remote::{
    map_http_status, read_body_limited_with_op, resolve_encoded_format, send_with_op,
    ExpectedWireFormat, HardenedHttpClient, RemoteBodyLimits, RemotePolicy, XaiHttpPolicy,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use crate::tts::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use crate::tts::validate::{clamp_speaking_rate, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;

const PROVIDER_NAME: &str = "xai";

/// Product label for official batch REST TTS (API has no model field).
pub const DEFAULT_XAI_TTS_MODEL: &str = "xai-tts";

/// Default built-in xAI voice.
pub const DEFAULT_XAI_TTS_VOICE: &str = "eve";

/// xAI TTS catalogue entry (product surface).
#[derive(Debug, Clone, Copy)]
pub struct XaiTtsRecord {
    pub model: &'static str,
    pub voices: &'static [&'static str],
    pub default_sample_rate_hz: u32,
    pub max_text_chars: usize,
    pub rate_min: f32,
    pub rate_max: f32,
}

/// Official built-in voices from GET /v1/tts/voices.
pub static XAI_BUILTIN_VOICES: &[&str] = &["eve", "ara", "leo", "rex", "sal"];

/// Static reviewed product IDs for xAI TTS (JOE-1976).
pub static XAI_TTS_REGISTRY: &[XaiTtsRecord] = &[XaiTtsRecord {
    model: "xai-tts",
    voices: XAI_BUILTIN_VOICES,
    default_sample_rate_hz: 24_000,
    max_text_chars: 15_000, // official max
    rate_min: 0.7,
    rate_max: 1.5,
}];

pub fn lookup_xai_tts(model: &str) -> Option<&'static XaiTtsRecord> {
    let m = model.trim();
    if m.is_empty() {
        return Some(&XAI_TTS_REGISTRY[0]);
    }
    XAI_TTS_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

#[derive(Debug, Serialize)]
struct XaiTtsRequest<'a> {
    text: &'a str,
    voice_id: &'a str,
    language: &'a str,
    output_format: XaiOutputFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f64>,
}

#[derive(Debug, Serialize)]
struct XaiOutputFormat {
    codec: &'static str,
    sample_rate: u32,
}

/// xAI batch REST speech provider.
pub struct XaiTtsProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    governor: Arc<ResourceGovernor>,
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

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, XaiHttpPolicy)?;
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

    fn resolve_model_voice(model: &str, voice: &str) -> Result<(&'static XaiTtsRecord, String)> {
        let rec = lookup_xai_tts(model).ok_or_else(|| UserError::UnsupportedCapability {
            provider: PROVIDER_NAME.into(),
            model: model.into(),
            reason: "model is not in the reviewed xAI TTS registry".into(),
            hint: format!(
                "use {} (official REST has no model id; do not use grok-tts* or OpenAI voices)",
                DEFAULT_XAI_TTS_MODEL
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
                reason: format!("voice '{voice}' is not a reviewed xAI built-in voice"),
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
        let op = opts.resolve_op_context();
        op.check()?;
        op.emit("tts", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
        op.check()?;
        op.emit("tts", "request");

        let sample_rate = rec.default_sample_rate_hz;
        let body = XaiTtsRequest {
            text,
            voice_id: &voice,
            language: "en",
            output_format: XaiOutputFormat {
                codec: "pcm",
                sample_rate,
            },
            speed: Some(rate as f64),
        };
        let json = serde_json::to_vec(&body).map_err(|e| ProviderError::Other {
            message: format!("xAI TTS request serialize: {e}"),
        })?;

        let response = send_with_op(
            self.http
                .request(reqwest::Method::POST, "tts", self.api_key.expose())?
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

        let expected = ExpectedWireFormat::pcm(sample_rate, 1);
        // Official batch TTS returns raw audio bytes (or JSON only when timestamps requested).
        let format = resolve_encoded_format(PROVIDER_NAME, expected, &content_type, &bytes)?;

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

        op.emit("tts", "done");
        Ok(SynthesisResult {
            pcm_i16_mono: norm.pcm_i16_mono,
            sample_rate_hz: norm.sample_rate_hz,
            channels: 1,
            backend_kind: BackendKind::Remote,
            provider: PROVIDER_NAME.into(),
            model: rec.model.into(),
            voice: voice.clone(),
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
    fn registry_and_voices() {
        assert!(lookup_xai_tts("xai-tts").is_some());
        assert!(lookup_xai_tts("grok-tts").is_none());
        assert!(lookup_xai_tts("").is_some());
        let err = XaiTtsProvider::resolve_model_voice("xai-tts", "alloy").unwrap_err();
        assert!(err.to_string().contains("voice") || err.to_string().contains("alloy"));
        assert!(XaiTtsProvider::resolve_model_voice("xai-tts", "eve").is_ok());
    }

    #[tokio::test]
    async fn mock_official_tts_pcm() {
        let server = MockServer::start().await;
        // 100ms mono s16le @ 24kHz = 4800 samples * 2 = 9600 bytes
        let pcm = vec![0u8; 9600];
        Mock::given(method("POST"))
            .and(path("/v1/tts"))
            .and(header("Authorization", "Bearer xai-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/pcm;rate=24000;channels=1")
                    .set_body_bytes(pcm),
            )
            .mount(&server)
            .await;

        let provider = XaiTtsProvider::with_policy(
            Some("xai-test".into()),
            Some(format!("{}/v1", server.uri().trim_end_matches('/'))),
            RemotePolicy {
                allow_loopback_http: true,
                allow_custom_credentialed_endpoint: true,
                ..Default::default()
            },
        )
        .unwrap();

        let opts = SynthesisOptions {
            model: "xai-tts".into(),
            voice: "eve".into(),
            timeout_ms: 5_000,
            ..Default::default()
        };
        let result = provider.synthesize("Hello from xAI", &opts).await.unwrap();
        assert_eq!(result.provider, "xai");
        assert_eq!(result.voice, "eve");
        assert_eq!(result.backend_kind, BackendKind::Remote);
        assert!(!result.pcm_i16_mono.is_empty());
    }

    #[tokio::test]
    async fn mock_rejects_json_as_pcm() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/tts"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/pcm")
                    .set_body_string(r#"{"error":"nope"}"#),
            )
            .mount(&server)
            .await;

        let provider = XaiTtsProvider::with_policy(
            Some("xai-test".into()),
            Some(format!("{}/v1", server.uri().trim_end_matches('/'))),
            RemotePolicy {
                allow_loopback_http: true,
                allow_custom_credentialed_endpoint: true,
                ..Default::default()
            },
        )
        .unwrap();

        let opts = SynthesisOptions {
            model: "xai-tts".into(),
            voice: "eve".into(),
            timeout_ms: 5_000,
            ..Default::default()
        };
        let err = provider.synthesize("Hi", &opts).await.unwrap_err();
        let s = err.to_string();
        assert!(!s.contains("nope"));
    }
}
