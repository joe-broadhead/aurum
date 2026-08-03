//! OpenRouter remote TTS via OpenAI-compatible `/audio/speech` (JOE-1939).
//!
//! Direction-specific from [`super::openrouter::OpenRouterProvider`] (STT).
//! Prefer PCM wire format and normalize through [`crate::audio::normalize_remote_audio`].

use crate::audio::{normalize_remote_audio, BoundedAudioBody, RemoteAudioLimits};
use crate::error::{ProviderError, Result, UserError};
use crate::remote::{
    map_http_status, read_body_limited_with_op, resolve_encoded_format, send_with_op,
    ExpectedWireFormat, HardenedHttpClient, OpenAiSpeechRequest, RemoteBodyLimits, RemotePolicy,
    SpeechResponseFormat,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use crate::tts::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use crate::tts::validate::{clamp_speaking_rate, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
use async_trait::async_trait;
use std::sync::Arc;

const PROVIDER_NAME: &str = "openrouter";

/// Default OpenRouter TTS model (live `output_modalities=speech` catalogue).
///
/// Support tier is **experimental**. The former OpenAI-family pin
/// `openai/gpt-4o-mini-tts-2025-12-15` was removed from OpenRouter (HTTP 400
/// "does not exist"); registry tracks currently reachable speech models only.
pub const DEFAULT_OPENROUTER_TTS_MODEL: &str = "hexgrad/kokoro-82m";

/// Default OpenAI-compatible voice id accepted by the default speech model.
pub const DEFAULT_OPENROUTER_TTS_VOICE: &str = "alloy";

/// Static evidence date for the reviewed registry (UTC calendar day).
pub const OPENROUTER_TTS_EVIDENCE_DATE: &str = "2026-08-02";

/// Support tier for a reviewed OpenRouter TTS record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRouterTtsTier {
    /// Protected live smoke retained; ordinary CI may use mocks.
    Supported,
    /// Implemented + mocks; no protected live evidence yet.
    Experimental,
    /// Requires deliberate model selection; not a default.
    ExplicitOnly,
}

/// Reviewed OpenRouter TTS catalogue entry (fail closed for unknown models).
#[derive(Debug, Clone, Copy)]
pub struct OpenRouterTtsRecord {
    pub model: &'static str,
    pub voices: &'static [&'static str],
    /// Nominal PCM sample rate when [`Self::response_format`] is PCM (must match
    /// Content-Type `rate=` when the vendor includes it).
    pub default_sample_rate_hz: u32,
    pub max_text_chars: usize,
    pub rate_min: f32,
    pub rate_max: f32,
    /// Wire `response_format` this vendor accepts (PCM preferred when available).
    pub response_format: SpeechResponseFormat,
    pub tier: OpenRouterTtsTier,
    /// UTC YYYY-MM-DD for last human/doc evidence note (not live discovery).
    pub evidence_date: &'static str,
    pub evidence_source: &'static str,
}

/// Static reviewed TTS models for OpenRouter (JOE-1939 / live speech catalogue).
///
/// None of these are promoted to [`OpenRouterTtsTier::Supported`] without a
/// protected credentialed speech smoke. Discovery via
/// `GET /api/v1/models?output_modalities=speech` is an explicit refresh workflow
/// (offline startup never fetches). Dead upstream ids are removed, not aliased.
pub static OPENROUTER_TTS_REGISTRY: &[OpenRouterTtsRecord] = &[
    OpenRouterTtsRecord {
        // Live speech catalogue; PCM 24 kHz; default experimental pin (2026-08-02).
        model: "hexgrad/kokoro-82m",
        voices: &["alloy"],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
        response_format: SpeechResponseFormat::Pcm,
        tier: OpenRouterTtsTier::Experimental,
        evidence_date: OPENROUTER_TTS_EVIDENCE_DATE,
        evidence_source:
            "models?output_modalities=speech + live POST /audio/speech PCM smoke (rate=24000)",
    },
    OpenRouterTtsRecord {
        model: "fish-audio/s1",
        voices: &["alloy"],
        default_sample_rate_hz: 44_100,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
        response_format: SpeechResponseFormat::Pcm,
        tier: OpenRouterTtsTier::Experimental,
        evidence_date: OPENROUTER_TTS_EVIDENCE_DATE,
        evidence_source: "live POST /audio/speech PCM smoke (rate=44100)",
    },
    OpenRouterTtsRecord {
        model: "sesame/csm-1b",
        voices: &["alloy"],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
        response_format: SpeechResponseFormat::Pcm,
        tier: OpenRouterTtsTier::Experimental,
        evidence_date: OPENROUTER_TTS_EVIDENCE_DATE,
        evidence_source: "live POST /audio/speech PCM smoke (rate=24000)",
    },
    OpenRouterTtsRecord {
        // MiniMax refuses PCM; MP3 only (streaming contract).
        model: "minimax/speech-2.8-turbo",
        voices: &["alloy"],
        default_sample_rate_hz: 24_000,
        max_text_chars: 4_096,
        rate_min: 0.25,
        rate_max: 4.0,
        response_format: SpeechResponseFormat::Mp3,
        tier: OpenRouterTtsTier::ExplicitOnly,
        evidence_date: OPENROUTER_TTS_EVIDENCE_DATE,
        evidence_source: "live POST /audio/speech MP3-only (PCM rejected by vendor)",
    },
];

pub fn lookup_openrouter_tts(model: &str) -> Option<&'static OpenRouterTtsRecord> {
    let m = model.trim();
    OPENROUTER_TTS_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// Whether `model_id` appears in a Models API speech-modality id list (bounded fixture).
pub fn openrouter_tts_model_in_discovery(model_id: &str, discovered_ids: &[&str]) -> bool {
    discovered_ids
        .iter()
        .any(|id| id.eq_ignore_ascii_case(model_id.trim()))
}

/// Remote OpenRouter TTS provider (speech endpoint only).
pub struct OpenRouterTtsProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for OpenRouterTtsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterTtsProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .finish()
    }
}

impl OpenRouterTtsProvider {
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

        let http = HardenedHttpClient::openrouter(base_url.as_deref(), policy)?;
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

    fn resolve_model_voice(
        model: &str,
        voice: &str,
    ) -> Result<(&'static OpenRouterTtsRecord, String)> {
        let rec = lookup_openrouter_tts(model).ok_or_else(|| UserError::UnsupportedCapability {
            provider: PROVIDER_NAME.into(),
            model: model.into(),
            reason: "model is not in the reviewed OpenRouter TTS registry".into(),
            hint: format!(
                "use one of: {}",
                OPENROUTER_TTS_REGISTRY
                    .iter()
                    .map(|r| r.model)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })?;
        let voice = voice.trim();
        if voice.is_empty() {
            return Ok((rec, DEFAULT_OPENROUTER_TTS_VOICE.into()));
        }
        // Never remap local Kitten/Kokoro aliases — only accept provider voice ids.
        let ok = rec.voices.iter().any(|v| v.eq_ignore_ascii_case(voice));
        if !ok {
            return Err(UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: model.into(),
                reason: format!("voice '{voice}' is not supported for this OpenRouter TTS model"),
                hint: format!("supported voices: {}", rec.voices.join(", ")),
            }
            .into());
        }
        // Canonical lowercase OpenAI voice ids.
        Ok((rec, voice.to_ascii_lowercase()))
    }
}

#[async_trait]
impl SynthesisProvider for OpenRouterTtsProvider {
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
                    "TTS text exceeds OpenRouter model limit ({} chars)",
                    rec.max_text_chars
                ),
            }
            .into());
        }

        // Intersect Aurum global rate range with model limits.
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
                hint: format!(
                    "use a rate in {}..={} (also within Aurum {}..={})",
                    rec.rate_min, rec.rate_max, SPEAKING_RATE_MIN, SPEAKING_RATE_MAX
                ),
            }
            .into());
        }
        let op = opts.resolve_op_context();
        op.check()?;
        op.emit("tts", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
        op.check()?;
        op.emit("tts", "request");

        let body =
            OpenAiSpeechRequest::new(&opts.model, text, &voice, rec.response_format, Some(rate));
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
        // Never echo response body text; status mapping is allowlisted only.
        map_http_status(PROVIDER_NAME, status, "")?;

        let expected = match rec.response_format {
            SpeechResponseFormat::Pcm => ExpectedWireFormat::pcm(rec.default_sample_rate_hz, 1),
            SpeechResponseFormat::Mp3 => ExpectedWireFormat::Mp3,
        };
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
        // Remote TTS has no local preload.
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
        assert!(lookup_openrouter_tts(DEFAULT_OPENROUTER_TTS_MODEL).is_some());
        assert!(lookup_openrouter_tts("not-a-real-model").is_none());
        // Dead OpenAI-family OpenRouter pins are not silently accepted.
        assert!(lookup_openrouter_tts("openai/gpt-4o-mini-tts").is_none());
        assert!(lookup_openrouter_tts("openai/gpt-4o-mini-tts-2025-12-15").is_none());
        let def = lookup_openrouter_tts(DEFAULT_OPENROUTER_TTS_MODEL).unwrap();
        assert_eq!(def.tier, OpenRouterTtsTier::Experimental);
        assert_eq!(def.response_format, SpeechResponseFormat::Pcm);
        assert!(openrouter_tts_model_in_discovery(
            "hexgrad/kokoro-82m",
            &["hexgrad/kokoro-82m", "fish-audio/s1"]
        ));
        let mp3 = lookup_openrouter_tts("minimax/speech-2.8-turbo").unwrap();
        assert_eq!(mp3.response_format, SpeechResponseFormat::Mp3);
    }

    #[test]
    fn rejects_local_voice_aliases() {
        let err = OpenRouterTtsProvider::resolve_model_voice(DEFAULT_OPENROUTER_TTS_MODEL, "Luna")
            .unwrap_err();
        assert!(err.to_string().contains("voice") || err.to_string().contains("Luna"));
    }

    #[tokio::test]
    async fn mock_pcm_speech_success() {
        let server = MockServer::start().await;
        // 100 ms @ 24 kHz mono s16le
        let n = 2_400;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = ((i % 50) as i16) * 20;
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

        let provider = OpenRouterTtsProvider::with_policy(
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
                "Hello from OpenRouter",
                &SynthesisOptions {
                    model: DEFAULT_OPENROUTER_TTS_MODEL.into(),
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

        assert_eq!(result.backend_kind, BackendKind::Remote);
        assert_eq!(result.provider, "openrouter");
        assert_eq!(result.voice, "alloy");
        assert_eq!(result.sample_rate_hz, 24_000);
        assert_eq!(result.pcm_i16_mono.len(), n);
        assert!(result.duration_ms >= 90 && result.duration_ms <= 110);
        assert!(result.adapter.is_none());
        assert!(result.trust.is_none());
    }

    #[tokio::test]
    async fn local_only_rejects_before_network() {
        let provider = OpenRouterTtsProvider::with_policy(
            Some("sk-test".into()),
            None,
            RemotePolicy::default(),
        )
        .unwrap();
        let err = provider
            .synthesize(
                "hi",
                &SynthesisOptions {
                    model: DEFAULT_OPENROUTER_TTS_MODEL.into(),
                    voice: "alloy".into(),
                    language: "en".into(),
                    sample_rate_hz: None,
                    speaking_rate: 1.0,
                    timeout_ms: 5_000,
                    cancel: None,
                    op: None,
                    local_only: true,
                    pack_dir: None,
                    allow_unverified: false,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("local_only") || err.to_string().contains("remote"));
    }
}
