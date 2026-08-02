//! ElevenLabs remote TTS (JOE-1941).
//!
//! `POST /v1/text-to-speech/{voice_id}?output_format=pcm_24000` with `xi-api-key`.
//! Voice IDs are provider-native; local Kitten aliases are never remapped.

use crate::audio::{normalize_remote_audio, BoundedAudioBody, RemoteAudioLimits};
use crate::error::{ProviderError, Result, UserError};
use crate::remote::{
    map_http_status, read_body_limited_with_op, resolve_encoded_format, send_with_op,
    ElevenLabsHttpPolicy, ExpectedWireFormat, HardenedHttpClient, RemoteBodyLimits, RemotePolicy,
};
use crate::runtime::{OpContext, PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use crate::tts::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use crate::tts::validate::{clamp_speaking_rate, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

const PROVIDER_NAME: &str = "elevenlabs";

/// Default reviewed ElevenLabs TTS model.
pub const DEFAULT_ELEVENLABS_TTS_MODEL: &str = "eleven_multilingual_v2";

/// Documented demo/public voice id (Rachel) for tests and static help only.
/// Operators should set their own `voice_id`; this is never implied from "Luna".
pub const EXAMPLE_ELEVENLABS_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";

/// Reviewed ElevenLabs model entry.
#[derive(Debug, Clone, Copy)]
pub struct ElevenLabsTtsRecord {
    pub model: &'static str,
    pub max_text_chars: usize,
    pub default_sample_rate_hz: u32,
    /// Query `output_format` for raw PCM mono s16le.
    pub pcm_output_format: &'static str,
    pub rate_min: f32,
    pub rate_max: f32,
}

pub static ELEVENLABS_TTS_REGISTRY: &[ElevenLabsTtsRecord] = &[
    ElevenLabsTtsRecord {
        model: "eleven_multilingual_v2",
        max_text_chars: 10_000,
        default_sample_rate_hz: 24_000,
        pcm_output_format: "pcm_24000",
        rate_min: 0.7,
        rate_max: 1.2,
    },
    ElevenLabsTtsRecord {
        model: "eleven_turbo_v2_5",
        max_text_chars: 40_000,
        default_sample_rate_hz: 24_000,
        pcm_output_format: "pcm_24000",
        rate_min: 0.7,
        rate_max: 1.2,
    },
    ElevenLabsTtsRecord {
        model: "eleven_flash_v2_5",
        max_text_chars: 40_000,
        default_sample_rate_hz: 24_000,
        pcm_output_format: "pcm_24000",
        rate_min: 0.7,
        rate_max: 1.2,
    },
];

pub fn lookup_elevenlabs_tts(model: &str) -> Option<&'static ElevenLabsTtsRecord> {
    let m = model.trim();
    ELEVENLABS_TTS_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// Validate voice_id shape: opaque provider id, not a local alias.
pub fn validate_elevenlabs_voice_id(voice: &str) -> Result<String> {
    let v = voice.trim();
    if v.is_empty() {
        return Err(UserError::Other {
            message: format!(
                "ElevenLabs requires an explicit voice_id (provider voice id).\n  \
                 Hint: pass --voice <id> (e.g. example {EXAMPLE_ELEVENLABS_VOICE_ID}); \
                 local names like Luna are not remapped."
            ),
        }
        .into());
    }
    // Reject obvious local catalogue names so we never pretend remapping works.
    let lower = v.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "luna" | "kate" | "hart" | "heart" | "nicole" | "default"
    ) {
        return Err(UserError::UnsupportedCapability {
            provider: PROVIDER_NAME.into(),
            model: "*".into(),
            reason: format!("'{v}' looks like a local Aurum voice alias, not an ElevenLabs voice_id"),
            hint: "use an ElevenLabs voice_id from your account (never remapped from Kitten/Kokoro names)"
                .into(),
        }
        .into());
    }
    if v.len() > 64 || v.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(UserError::Other {
            message: "invalid ElevenLabs voice_id".into(),
        }
        .into());
    }
    Ok(v.to_string())
}

/// Percent-encode a path segment (no `/` unescaped). Does not log the raw value.
fn urlencoding_path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Serialize)]
struct ElevenLabsSpeechBody<'a> {
    text: &'a str,
    model_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_settings: Option<VoiceSettings>,
}

#[derive(Debug, Serialize)]
struct VoiceSettings {
    /// Mapped from Aurum speaking_rate into stability-adjacent speed when used.
    /// ElevenLabs uses `speed` on some models via voice_settings or top-level;
    /// we send a conservative voice_settings.speed when rate != 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f32>,
}

/// ElevenLabs TTS provider (xi-api-key).
pub struct ElevenLabsTtsProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for ElevenLabsTtsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevenLabsTtsProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .finish()
    }
}

impl ElevenLabsTtsProvider {
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

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, ElevenLabsHttpPolicy)?;
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
}

#[async_trait]
impl SynthesisProvider for ElevenLabsTtsProvider {
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

        let rec =
            lookup_elevenlabs_tts(&opts.model).ok_or_else(|| UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: opts.model.clone(),
                reason: "model is not in the reviewed ElevenLabs TTS registry".into(),
                hint: format!(
                    "use one of: {}",
                    ELEVENLABS_TTS_REGISTRY
                        .iter()
                        .map(|r| r.model)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })?;

        if text.chars().count() > rec.max_text_chars {
            return Err(UserError::Other {
                message: format!(
                    "TTS text exceeds ElevenLabs model limit ({} chars)",
                    rec.max_text_chars
                ),
            }
            .into());
        }

        let voice_id = validate_elevenlabs_voice_id(&opts.voice)?;

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
        op.emit("tts", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
        op.check()?;
        op.emit("tts", "request");

        // Encode voice_id as a single path segment (JOE-1979) — never raw interpolate.
        let voice_seg = urlencoding_path_segment(&voice_id);
        let path = format!(
            "v1/text-to-speech/{voice_seg}?output_format={}",
            rec.pcm_output_format
        );
        let body = ElevenLabsSpeechBody {
            text,
            model_id: rec.model,
            voice_settings: if (rate - 1.0).abs() > 0.001 {
                Some(VoiceSettings { speed: Some(rate) })
            } else {
                None
            },
        };
        let json = serde_json::to_vec(&body).map_err(|e| ProviderError::Other {
            message: format!("elevenlabs request serialize: {e}"),
        })?;

        let response = send_with_op(
            self.http
                .request(reqwest::Method::POST, &path, self.api_key.expose())?
                .header("Content-Type", "application/json")
                .header("Accept", "audio/pcm")
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

        // Requested pcm_24000; accept only allowlisted PCM MIME (JOE-1977).
        let expected = ExpectedWireFormat::pcm(rec.default_sample_rate_hz, 1);
        let format = if content_type.to_ascii_lowercase().contains("mpeg")
            || content_type.to_ascii_lowercase().contains("audio/mp3")
        {
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
            voice: voice_id,
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
    use wiremock::matchers::{header, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn registry_and_voice_validation() {
        assert!(lookup_elevenlabs_tts(DEFAULT_ELEVENLABS_TTS_MODEL).is_some());
        assert!(validate_elevenlabs_voice_id(EXAMPLE_ELEVENLABS_VOICE_ID).is_ok());
        assert!(validate_elevenlabs_voice_id("Luna").is_err());
        assert!(validate_elevenlabs_voice_id("").is_err());
    }

    #[tokio::test]
    async fn mock_pcm_tts() {
        let server = MockServer::start().await;
        let n = 2_400;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = ((i % 30) as i16) * 15;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/text-to-speech/[^/]+$"))
            .and(header("xi-api-key", "el-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "audio/pcm")
                    .set_body_bytes(pcm),
            )
            .mount(&server)
            .await;

        let provider = ElevenLabsTtsProvider::with_policy(
            Some("el-test".into()),
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
                "Hello ElevenLabs",
                &SynthesisOptions {
                    model: DEFAULT_ELEVENLABS_TTS_MODEL.into(),
                    voice: EXAMPLE_ELEVENLABS_VOICE_ID.into(),
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
        assert_eq!(result.provider, "elevenlabs");
        assert_eq!(result.backend_kind, BackendKind::Remote);
        assert_eq!(result.sample_rate_hz, 24_000);
        assert_eq!(result.pcm_i16_mono.len(), n);
        assert_eq!(result.voice, EXAMPLE_ELEVENLABS_VOICE_ID);
    }
}
