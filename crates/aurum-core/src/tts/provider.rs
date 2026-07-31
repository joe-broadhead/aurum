//! TTS provider trait and shared result types.

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// Deserialize remains for BackendKind / options enums only; SynthesisResult is
// Serialize-only (JOE-1614).

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
    /// Optional requested sample rate. Must equal the adapter native rate when set;
    /// non-native values are rejected (no metadata-only relabeling, no resampler).
    pub sample_rate_hz: Option<u32>,
    /// Speaking rate multiplier (must be finite and within engine range).
    pub speaking_rate: f32,
    /// Wall-clock timeout in milliseconds (enforced by the local provider).
    pub timeout_ms: u64,
    /// Optional cooperative cancel flag.
    pub cancel: Option<crate::cancel::CancelFlag>,
    /// When true, never hit the network for missing voice packs.
    pub local_only: bool,
    /// Optional local model-pack directory (JOE-1619). When set, loads artifacts
    /// from the pack manifest instead of the built-in catalogue cache. Requires a
    /// known adapter; bare ONNX paths are rejected by pack load.
    pub pack_dir: Option<std::path::PathBuf>,
    /// Allow `local_unverified` trust for [`Self::pack_dir`] (explicit opt-in).
    pub allow_unverified: bool,
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
            pack_dir: None,
            allow_unverified: false,
        }
    }
}

/// Normalized result returned by every TTS provider.
///
/// **Not** deserializable as a whole: PCM must not be reconstructed from JSON
/// metadata (JOE-1614). Use [`crate::dto::TtsMetaDto`] for external contracts.
#[derive(Debug, Clone, Serialize)]
pub struct SynthesisResult {
    /// Mono PCM samples (signed 16-bit).
    #[serde(skip)]
    pub pcm_i16_mono: Vec<i16>,
    /// Actual sample rate of [`Self::pcm_i16_mono`] (always the adapter native rate).
    pub sample_rate_hz: u32,
    /// Always 1 for MVP.
    pub channels: u16,
    pub backend_kind: BackendKind,
    pub provider: String,
    /// Canonical model id actually used.
    pub model: String,
    /// Canonical voice id actually used.
    pub voice: String,
    pub language: String,
    /// Duration derived from final PCM length and actual sample rate.
    pub duration_ms: u64,
    /// Character count of the synthesized (complete) text.
    pub text_chars: usize,
    /// Always `false` under complete-or-error policy.
    pub text_truncated: bool,
    /// Number of model-safe chunks synthesized and concatenated.
    pub chunk_count: usize,
    /// Characters of source text actually synthesized (equals `text_chars`).
    pub synthesized_chars: usize,
    /// Adapter id that executed synthesis (JOE-1576).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    /// Trust mode: builtin | verified | local_unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<String>,
    /// Provenance: builtin | custom | local_pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

/// Provider trait for local (and future) TTS backends.
#[async_trait]
pub trait SynthesisProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult>;

    async fn preload(&self, model: &str, voice: &str) -> Result<()>;
}
