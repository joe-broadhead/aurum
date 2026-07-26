//! TTS provider trait and shared result types.

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Default sample rate produced by the KittenTTS nano engine.
pub const DEFAULT_SAMPLE_RATE_HZ: u32 = 24_000;

/// Backend classification for honesty JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// On-device ONNX inference.
    Local,
}

/// Options controlling a single synthesis request.
#[derive(Debug, Clone)]
pub struct SynthesisOptions {
    pub model: String,
    pub voice: String,
    pub language: String,
    /// Optional override; engine may ignore if fixed by the model.
    pub sample_rate_hz: Option<u32>,
    /// Speaking rate multiplier (clamped by callers, e.g. 0.5..=2.0).
    pub speaking_rate: f32,
    /// Wall-clock timeout in milliseconds (enforced by the local provider).
    pub timeout_ms: u64,
    /// Optional cooperative cancel flag.
    pub cancel: Option<crate::cancel::CancelFlag>,
    /// When true, never hit the network for missing voice packs.
    pub local_only: bool,
}

impl Default for SynthesisOptions {
    fn default() -> Self {
        Self {
            model: crate::tts::DEFAULT_TTS_MODEL.to_string(),
            voice: crate::tts::DEFAULT_TTS_VOICE.to_string(),
            language: "en".to_string(),
            sample_rate_hz: None,
            speaking_rate: 1.0,
            timeout_ms: crate::tts::DEFAULT_TIMEOUT_MS,
            cancel: None,
            local_only: false,
        }
    }
}

/// Normalized result returned by every TTS provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    /// Mono PCM samples (signed 16-bit).
    #[serde(skip)]
    pub pcm_i16_mono: Vec<i16>,
    pub sample_rate_hz: u32,
    /// Always 1 for MVP.
    pub channels: u16,
    pub backend_kind: BackendKind,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub language: String,
    pub duration_ms: u64,
    pub text_chars: usize,
    pub text_truncated: bool,
}

/// Provider trait for local (and future) TTS backends.
#[async_trait]
pub trait SynthesisProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult>;

    async fn preload(&self, model: &str, voice: &str) -> Result<()>;
}
