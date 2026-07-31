//! Local text-to-speech (TTS) — ONNX KittenTTS + MIT G2P.
//!
//! Enabled by the `tts` cargo feature (default on). Produces mono PCM that the
//! CLI writes as WAV. No remote TTS, no ffmpeg, no GPL-linked phonemizer.
//!
//! Correctness contract (v0.0.3 / JOE-1571):
//! - complete-or-error input (no silent truncation)
//! - model-aware phoneme chunking with documented inter-chunk pause
//! - truthful native sample rate and duration from final PCM
//! - signal-aware trailing-silence trim (never empty valid short output)
//! - model-scoped voice selection
//! - shared secure output transaction for WAV commits
//!
//! Model platform (v0.0.3 / JOE-1576):
//! - versioned adapter contract + pack manifests (not bare ONNX paths)
//! - trust modes: builtin | verified | local_unverified
//! - conformance suite for adapters/packs
//! - custom catalogue entries + inspect/verify/add tooling

#[cfg(feature = "tts")]
pub mod adapter;
#[cfg(feature = "tts")]
pub mod byom;
#[cfg(feature = "tts")]
pub mod catalogue;
#[cfg(feature = "tts")]
mod chunk;
#[cfg(feature = "tts")]
pub mod conformance;
#[cfg(feature = "tts")]
pub mod custom;
#[cfg(feature = "tts")]
pub mod local;
#[cfg(feature = "tts")]
mod npz;
#[cfg(feature = "tts")]
pub mod pack;
#[cfg(feature = "tts")]
mod pcm_post;
#[cfg(feature = "tts")]
pub mod provider;
#[cfg(feature = "tts")]
mod tokenize;
#[cfg(feature = "tts")]
pub mod validate;
#[cfg(feature = "tts")]
pub mod wav;

#[cfg(feature = "tts")]
pub use adapter::{
    list_adapters, lookup_adapter, preflight_manifest, known_adapter_id, AdapterDescriptor,
    ManifestArtifact, ManifestVoice, ModelPackManifest, TrustMode, ADAPTER_FAKE_SINE_V1,
    ADAPTER_KITTEN_ONNX_V1, ADAPTER_KOKORO_ONNX_V0, MANIFEST_SCHEMA_VERSION,
};
#[cfg(feature = "tts")]
pub use byom::{
    builtin_kitten_manifest_json, format_adapters, format_inspect, inspect_pack, propose_add_local,
    verify_pack, write_add_manifest, AddProposal, InspectReport,
};
#[cfg(feature = "tts")]
pub use catalogue::{
    ensure_voice_pack, format_model_list, format_voice_list, list_models, list_voices, lookup_model,
    lookup_voice, resolve_voice_for_model, tts_cache_dir, ModelStatus, VoiceInfo, VoiceStatus,
    DEFAULT_TTS_MODEL, DEFAULT_TTS_VOICE, PLACEHOLDER_ADAPTER_MODEL,
};
#[cfg(feature = "tts")]
pub use conformance::{
    adapter_registry_complete, kitten_builtin_manifest, run_kitten_catalogue_conformance,
    run_pack_conformance, synthesize_fake_sine_ms, ConformanceFailure, ConformanceReport,
};
#[cfg(feature = "tts")]
pub use custom::{
    format_custom_list, list_custom_status, validate_custom_models, CustomModelStatus,
    CustomTtsModel, CustomTtsModelEntry, MAX_CUSTOM_MODELS,
};
#[cfg(feature = "tts")]
pub use local::LocalTtsProvider;
#[cfg(feature = "tts")]
pub use pack::{
    custom_pack_cache_dir, load_pack_dir, sha256_file, verify_pack_artifacts, write_fake_sine_pack,
    write_manifest, MANIFEST_FILENAME, MAX_ARTIFACT_BYTES,
};
#[cfg(feature = "tts")]
pub use provider::{
    BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult, DEFAULT_SAMPLE_RATE_HZ,
};
#[cfg(feature = "tts")]
pub use validate::{
    clamp_speaking_rate, normalize_tts_language, prepare_text, resolve_sample_rate,
    tts_input_byte_budget, validate_output_path, validate_text, PreparedText, DEFAULT_MAX_CHARS,
    DEFAULT_TIMEOUT_MS, SPEAKING_RATE_MAX, SPEAKING_RATE_MIN,
};
#[cfg(feature = "tts")]
pub use wav::{write_wav_i16_mono_atomic, write_wav_i16_mono_transaction};
