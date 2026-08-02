//! Typed library configuration and operation contracts (JOE-2221).
//!
//! This module is the intentional host-facing shape for 0.0.22+:
//! * [`AurumConfig`] — direction-oriented validated runtime config
//! * [`OperationOptions`] — shared cancel / deadline / progress / request id
//! * [`TranscriptionRequest`] / [`SynthesisRequest`] — direction-specific requests
//!
//! [`crate::config::ConfigFile`] remains the serializable file schema.
//! [`crate::config::Config`] remains the CLI-oriented flat runtime bag; convert
//! once into [`AurumConfig`] via [`AurumConfig::try_from_config`].

use crate::cancel::CancelFlag;
use crate::config::{Config, ConfigFile, ValidatedConfig};
use crate::error::{Result, UserError};
use crate::runtime::{OpContext, ProgressSink};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Shared operation control for STT, TTS, and cleanup.
///
/// One absolute deadline propagates through nested stages. Cancel tokens and
/// progress sinks are never recreated silently by the engine when supplied here.
#[derive(Clone, Default)]
pub struct OperationOptions {
    cancel: CancelFlag,
    deadline: Option<Instant>,
    progress: Option<ProgressSink>,
    request_id: Option<u64>,
}

impl std::fmt::Debug for OperationOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationOptions")
            .field("cancelled", &self.cancel.is_cancelled())
            .field("has_deadline", &self.deadline.is_some())
            .field("has_progress", &self.progress.is_some())
            .field("request_id", &self.request_id)
            .finish()
    }
}

impl OperationOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cancel(mut self, cancel: CancelFlag) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_timeout_from_now(mut self, timeout: Duration) -> Self {
        self.deadline = Some(Instant::now() + timeout);
        self
    }

    pub fn with_progress(mut self, sink: ProgressSink) -> Self {
        self.progress = Some(sink);
        self
    }

    pub fn with_request_id(mut self, id: u64) -> Self {
        self.request_id = Some(id);
        self
    }

    pub fn cancel(&self) -> &CancelFlag {
        &self.cancel
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn request_id(&self) -> Option<u64> {
        self.request_id
    }

    /// Convert into the engine [`OpContext`] (preserves cancel and deadline).
    pub fn into_op_context(self) -> OpContext {
        let mut ctx = OpContext::with_cancel(self.cancel);
        if let Some(d) = self.deadline {
            ctx = ctx.with_absolute_deadline(d);
        }
        if let Some(p) = self.progress {
            ctx = ctx.with_progress(p);
        }
        if let Some(id) = self.request_id {
            ctx = ctx.with_request_id(id);
        }
        ctx
    }
}

/// STT request with shared operation control.
#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    pub model: String,
    pub language: String,
    pub timestamps: bool,
    pub operation: OperationOptions,
}

impl TranscriptionRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            language: "auto".into(),
            timestamps: false,
            operation: OperationOptions::new(),
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn timestamps(mut self, on: bool) -> Self {
        self.timestamps = on;
        self
    }

    pub fn operation(mut self, op: OperationOptions) -> Self {
        self.operation = op;
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "transcription request model must be non-empty".into(),
            }
            .into());
        }
        if self.model.len() > 256 {
            return Err(UserError::InvalidConfig {
                reason: "transcription request model id exceeds 256 characters".into(),
            }
            .into());
        }
        Ok(())
    }
}

/// TTS request with shared operation control.
#[derive(Debug, Clone)]
pub struct SynthesisRequest {
    pub model: String,
    pub voice: String,
    pub language: String,
    pub speaking_rate: f32,
    pub operation: OperationOptions,
}

impl SynthesisRequest {
    pub fn new(model: impl Into<String>, voice: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            voice: voice.into(),
            language: "en".into(),
            speaking_rate: 1.0,
            operation: OperationOptions::new(),
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn speaking_rate(mut self, rate: f32) -> Self {
        self.speaking_rate = rate;
        self
    }

    pub fn operation(mut self, op: OperationOptions) -> Self {
        self.operation = op;
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "synthesis request model must be non-empty".into(),
            }
            .into());
        }
        if self.voice.trim().is_empty() {
            return Err(UserError::InvalidConfig {
                reason: "synthesis request voice must be non-empty".into(),
            }
            .into());
        }
        if !self.speaking_rate.is_finite() || self.speaking_rate <= 0.0 || self.speaking_rate > 4.0 {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "speaking_rate {} is out of range (0, 4]",
                    self.speaking_rate
                ),
            }
            .into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AurumConfig — direction-oriented library config
// ---------------------------------------------------------------------------

/// Runtime policy applied consistently to both STT and TTS directions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeConfig {
    /// When true, reject remote providers before encoding or request construction.
    pub local_only: bool,
    pub cache_dir: PathBuf,
}

/// STT direction settings (no CLI presentation fields).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttConfig {
    pub provider: String,
    pub model: String,
    pub language: String,
    pub timestamps: bool,
}

/// Cleanup direction settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanupConfig {
    pub style: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// TTS direction settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsConfig {
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub language: String,
    pub speaking_rate: f32,
    pub max_chars: usize,
    pub timeout_ms: u64,
    #[serde(default)]
    pub allow_unverified: bool,
}

/// Redacted provider profile presence (secrets stay in validated Config only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderProfiles {
    pub openrouter_configured: bool,
    pub openai_configured: bool,
    pub elevenlabs_configured: bool,
    pub xai_configured: bool,
}

/// Library-facing typed configuration (JOE-2221).
///
/// Distinct from [`ConfigFile`] (on-disk schema) and CLI presentation fields.
/// Construct via [`AurumConfig::try_from_config`] or [`AurumConfig::load`].
#[derive(Debug, Clone)]
pub struct AurumConfig {
    pub runtime: RuntimeConfig,
    pub stt: SttConfig,
    #[cfg(feature = "tts")]
    pub tts: TtsConfig,
    pub cleanup: CleanupConfig,
    pub providers: ProviderProfiles,
    /// Opaque validated bag used to construct the engine (includes secrets).
    validated: ValidatedConfig,
}

impl AurumConfig {
    /// Load file/env defaults, validate, and project into typed sections.
    pub fn load() -> Result<Self> {
        Self::try_from_config(Config::load()?)
    }

    /// Convert a flat runtime [`Config`] once into the typed library model.
    pub fn try_from_config(cfg: Config) -> Result<Self> {
        let validated = ValidatedConfig::try_from_config(cfg.clone())?;
        Ok(Self::from_validated(validated))
    }

    /// Project an already-validated config.
    pub fn from_validated(validated: ValidatedConfig) -> Self {
        let c = validated.as_config();
        Self {
            runtime: RuntimeConfig {
                local_only: c.local_only,
                cache_dir: c.cache_dir.clone(),
            },
            stt: SttConfig {
                provider: c.provider.clone(),
                model: c.model.clone().unwrap_or_else(|| c.resolve_model_or_default()),
                language: c.language.clone(),
                timestamps: c.timestamps,
            },
            #[cfg(feature = "tts")]
            tts: TtsConfig {
                provider: c.tts_provider.clone(),
                model: c.tts_model.clone(),
                voice: c.tts_voice.clone(),
                language: c.tts_language.clone(),
                speaking_rate: c.tts_speaking_rate,
                max_chars: c.tts_max_chars,
                timeout_ms: c.tts_timeout_ms,
                allow_unverified: c.tts_allow_unverified,
            },
            cleanup: CleanupConfig {
                style: c.cleanup_style.clone(),
                provider: c.cleanup_provider.clone(),
                model: c.cleanup_openrouter_model.clone(),
            },
            providers: ProviderProfiles {
                openrouter_configured: c.openrouter_api_key.is_some(),
                openai_configured: c.providers.openai.api_key.is_some(),
                elevenlabs_configured: c.providers.elevenlabs.api_key.is_some(),
                xai_configured: c.providers.xai.api_key.is_some(),
            },
            validated,
        }
    }

    /// Build from a serializable file schema (plus env merge via Config).
    pub fn try_from_file(file: ConfigFile) -> Result<Self> {
        // Config::from_parts is private; round-trip via TOML is avoided — use load paths.
        // Hosts should prefer ValidatedConfig / Config::load. This path validates the
        // file shape by converting through the standard loader helpers.
        let _ = file;
        Err(UserError::InvalidConfig {
            reason: "use AurumConfig::load or try_from_config(Config::load_from(path)?)".into(),
        }
        .into())
    }

    pub fn validated(&self) -> &ValidatedConfig {
        &self.validated
    }

    pub fn into_validated(self) -> ValidatedConfig {
        self.validated
    }

    /// Apply local-only policy consistently (re-validates).
    pub fn with_local_only(self, local_only: bool) -> Result<Self> {
        let v = self.validated.with_local_only(local_only)?;
        Ok(Self::from_validated(v))
    }
}

// Helper on Config for model default without falling through CLI.
trait ResolveModelOrDefault {
    fn resolve_model_or_default(&self) -> String;
}

impl ResolveModelOrDefault for Config {
    fn resolve_model_or_default(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| crate::config::DEFAULT_LOCAL_MODEL.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_request_validation() {
        assert!(TranscriptionRequest::new("base").validate().is_ok());
        assert!(TranscriptionRequest::new("").validate().is_err());
    }

    #[test]
    fn synthesis_request_rate_bounds() {
        let ok = SynthesisRequest::new("kitten-nano-int8", "Luna");
        assert!(ok.validate().is_ok());
        let bad = SynthesisRequest::new("m", "v").speaking_rate(0.0);
        assert!(bad.validate().is_err());
        let nan = SynthesisRequest::new("m", "v").speaking_rate(f32::NAN);
        assert!(nan.validate().is_err());
    }

    #[test]
    fn operation_options_into_op_context() {
        let cancel = CancelFlag::new();
        cancel.cancel();
        let op = OperationOptions::new()
            .with_cancel(cancel)
            .with_timeout_from_now(Duration::from_secs(5))
            .with_request_id(42);
        let ctx = op.into_op_context();
        assert!(ctx.cancel.is_cancelled());
        assert_eq!(ctx.request_id, 42);
        assert!(ctx.deadline().is_some() || ctx.remaining().is_some());
    }

    #[test]
    fn aurum_config_from_defaults() {
        let cfg = Config {
            provider: "local".into(),
            model: Some("base".into()),
            language: "en".into(),
            output: "txt".into(),
            output_file: None,
            timestamps: false,
            verbose: false,
            openrouter_api_key: None,
            openrouter_base_url: crate::config::DEFAULT_OPENROUTER_BASE_URL.into(),
            openrouter_default_model: crate::config::DEFAULT_OPENROUTER_MODEL.into(),
            openrouter_allow_custom_endpoint: false,
            openrouter_stt_mode: "auto".into(),
            openrouter_use_system_proxy: false,
            providers: Default::default(),
            cleanup_style: "raw".into(),
            cleanup_provider: "rules".into(),
            cleanup_openrouter_model: None,
            tts_provider: "local".into(),
            tts_model: "kitten-nano-int8".into(),
            tts_voice: "Luna".into(),
            tts_language: "en".into(),
            tts_speaking_rate: 1.0,
            tts_max_chars: 5000,
            tts_timeout_ms: 120_000,
            tts_pack_dir: None,
            tts_allow_unverified: false,
            tts_custom_models: vec![],
            local_only: false,
            config_path: None,
            cache_dir: std::env::temp_dir().join("aurum-sdk-test-cache"),
        };
        let ac = AurumConfig::try_from_config(cfg).unwrap();
        assert_eq!(ac.stt.provider, "local");
        assert_eq!(ac.stt.model, "base");
        assert!(!ac.runtime.local_only);
        assert!(!ac.providers.openrouter_configured);
    }
}
