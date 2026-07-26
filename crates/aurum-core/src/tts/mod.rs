//! Local text-to-speech (TTS) — ONNX KittenTTS + MIT G2P.
//!
//! Enabled by the `tts` cargo feature (default on). Produces mono PCM that the
//! CLI writes as WAV. No remote TTS, no ffmpeg, no GPL-linked phonemizer.

#[cfg(feature = "tts")]
pub mod catalogue;
#[cfg(feature = "tts")]
pub mod local;
#[cfg(feature = "tts")]
mod npz;
#[cfg(feature = "tts")]
pub mod provider;
#[cfg(feature = "tts")]
mod tokenize;
#[cfg(feature = "tts")]
pub mod validate;
#[cfg(feature = "tts")]
pub mod wav;

#[cfg(feature = "tts")]
pub use catalogue::{
    ensure_voice_pack, format_model_list, format_voice_list, list_models, list_voices,
    lookup_model, lookup_voice, tts_cache_dir, ModelStatus, VoiceInfo, VoiceStatus,
    DEFAULT_TTS_MODEL, DEFAULT_TTS_VOICE,
};
#[cfg(feature = "tts")]
pub use local::LocalTtsProvider;
#[cfg(feature = "tts")]
pub use provider::{
    BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult, DEFAULT_SAMPLE_RATE_HZ,
};
#[cfg(feature = "tts")]
pub use validate::{
    clamp_speaking_rate, normalize_tts_language, prepare_text, tts_input_byte_budget,
    validate_output_path, validate_text, PreparedText, DEFAULT_MAX_CHARS, DEFAULT_TIMEOUT_MS,
    SPEAKING_RATE_MAX, SPEAKING_RATE_MIN,
};
#[cfg(feature = "tts")]
pub use wav::write_wav_i16_mono_atomic;
