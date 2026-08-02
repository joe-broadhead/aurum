//! # aurum-core
//!
//! Reusable **on-device speech I/O** library (experimental API).
//!
//! - **STT** — local whisper.cpp by default; optional OpenRouter
//! - **TTS** — local ONNX KittenTTS (cargo feature `tts`, default on)
//! - **Cleanup** — rules or optional LLM post-edit
//!
//! Tagline: *Speech both ways. On-device by default.*
//!
//! The API may change without notice until a deliberate major version.
//!
//! ## STT example (provider path)
//!
//! ```rust,no_run
//! use aurum_core::audio::{AudioInput, WHISPER_SAMPLE_RATE};
//! use aurum_core::pcm::PcmBuffer;
//! use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
//! use std::path::PathBuf;
//!
//! # async fn demo() -> aurum_core::error::Result<()> {
//! let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
//!     .with_progress(false)
//!     .with_local_only(false);
//! provider.preload("tiny-q5_1").await?;
//!
//! let mut buf = PcmBuffer::dictation();
//! buf.push(&[0.0f32; 1600])?;
//! let result = provider
//!     .transcribe_pcm(
//!         buf.samples().as_slice(),
//!         &TranscriptionOptions {
//!             model: "tiny-q5_1".into(),
//!             language: "en".into(),
//!             timestamps: false,
//!             cancel: None,
//!         },
//!     )
//!     .await?;
//! let _ = AudioInput::from_pcm_slice(buf.samples().as_slice(), WHISPER_SAMPLE_RATE)?;
//! println!("{}", result.text());
//! aurum_core::providers::local::clear_context_cache();
//! # Ok(())
//! # }
//! ```
//!
//! ## Engine path (preferred for library hosts)
//!
//! ```rust,no_run
//! use aurum_core::AurumEngine;
//!
//! # fn demo() -> aurum_core::error::Result<()> {
//! let engine = AurumEngine::load()?;
//! let _ = engine.doctor();
//! let _ = engine.support_bundle(None);
//! // engine.transcribe_pcm(&samples, &opts).await?; // STT on engine pools
//! engine.shutdown(); // closes + clears idle models in *this* engine only
//! # Ok(())
//! # }
//! ```

pub mod audio;
pub mod batch;
pub mod bench;
pub mod cache;
pub mod cancel;
pub mod capabilities;
pub mod cleanup;
pub mod config;
pub mod doctor;
pub mod domain;
pub mod download;
pub mod dto;
pub mod engine;
pub mod error;
pub mod eval;
pub mod model;
pub mod observability;
pub mod output;
pub mod partial;
pub mod pcm;
pub mod postprocess;
pub mod prelude;
pub mod product_contracts;
pub mod profile;
pub mod provider_platform;
pub mod providers;
pub mod remote;
pub mod runtime;
pub mod sdk;
pub mod secret;
pub mod support;
#[cfg(feature = "tts")]
pub mod tts;
pub mod window;

pub use audio::{
    load_audio, normalize_remote_audio, try_load_wav_file, AudioInput, BoundedAudioBody,
    ChannelPolicy, EncodedAudioFormat, NormalizedAudio, RemoteAudioLimits, ALLOWED_SAMPLE_RATES_HZ,
    DEFAULT_MAX_DURATION, DEFAULT_MAX_ENCODED_BYTES, DEFAULT_MAX_PCM_SAMPLES, WHISPER_SAMPLE_RATE,
};
pub use batch::{
    acquire_batch_lock, build_items, discover_inputs, discovery_preflight_id, lock_path,
    manifest_path, merge_for_resume, operation_fingerprint, prepare_resume, sha256_file_full,
    truncate_error, validate_batch_stt_provider, verify_item_for_resume, work_indices, BatchItem,
    BatchItemStatus, BatchLock, BatchLockGuard, BatchManifest, BatchSummary,
    OperationFingerprintInput, ResumeDecision, AUDIO_EXTENSIONS, BATCH_LOCK_NAME,
    BATCH_MANIFEST_NAME, BATCH_MANIFEST_VERSION, BATCH_MANIFEST_VERSION_V1, MAX_BATCH_ERROR_CHARS,
};
pub use cancel::CancelFlag;
pub use capabilities::{
    apply_stt_request_gates, lookup_openrouter_stt, preflight_cleanup, preflight_cleanup_for,
    preflight_stt, preflight_stt_for, preflight_tts, preflight_tts_for, CapabilityOperation,
    DescriptorFreshness, OpenRouterSttPath, OpenRouterSttRecord, ProviderCapabilities,
    SttBackendClass, VoiceModel, CAPABILITY_SCHEMA_VERSION, OPENROUTER_STT_REGISTRY,
};
pub use cleanup::{
    apply_cleanup, apply_cleanup_with_segments, apply_cleanup_with_segments_op, cleanup_text,
    CleanupProviderKind, CleanupReport, CleanupResult, CleanupStyle, OpenRouterCleanup,
    RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};
pub use config::{Config, ConfigFile, EffectiveConfigDiagnostic, ValidatedConfig};
pub use doctor::{run_doctor, DoctorCheck, DoctorReport, DoctorSeverity, DOCTOR_SCHEMA_VERSION};
pub use domain::{FiniteDurationSecs, ModelId, SampleRateHz};
pub use dto::{ErrorDto, SttResultDto, ERROR_SCHEMA_VERSION, STT_RESULT_SCHEMA_VERSION};
pub use engine::AurumEngine;
pub use error::{AurumError, ErrorCategory, Result, TranscriptionError};
pub use eval::{
    aggregate_listening, allowed_mean_wer, budget_exit_code, build_report, char_error_rate,
    compare_perf_budget, compare_stt_budget, evaluate_support_tier, join_discontinuity_score,
    observatory_core_budget_tiny, observatory_core_corpus, percentile_sorted,
    perf_budget_exit_code, perf_scenario_catalogue, repetition_ratio, score_observatory_fixture,
    score_stt, score_tts_pcm, silence_false_positive, smoke_corpus, tier_a_profile_templates,
    tts_local_matrix, tts_production_pack, word_error_rate, AssetResolution, BudgetComparison,
    BudgetFinding, BudgetSeverity, CorpusCoverage, EvalCorpus, EvalReport, HardwareTier,
    ListeningAggregate, ListeningRating, ListeningReport, NamedHardwareProfile, ObservatoryCorpus,
    ObservatoryFixture, ObservatoryFixtureScore, ObservatoryReport, ObservatoryScoreExtras,
    PerfBudget, PerfComparison, PerfFinding, PerfReport, PerfScenario, PerfScenarioBudget,
    PerfScenarioResult, PerfSeverity, RunIdentity, SttBudget, SttFixture, SttScore,
    SupportTierDecision, TtsEvalFixture, TtsEvalPack, TtsEvalParticipation, TtsObjectiveReport,
    TtsObjectiveScore, TtsObjectiveThresholds, TtsRunIdentity, NORMALIZATION_POLICY_VERSION,
    OBSERVATORY_SCHEMA_VERSION, PERF_EVIDENCE_VERSION, PERF_SCHEMA_VERSION,
    STT_OBSERVATORY_EVIDENCE_VERSION, TTS_EVAL_SCHEMA_VERSION, TTS_EVIDENCE_VERSION,
};
pub use model::{list_models, DownloadProgress, EnsureModelOptions, ModelInfo, ModelStatus};
pub use observability::{
    privacy_scan, process_metrics, BoundedEventSink, DiagnosticBundle, EventSink, Metrics,
    MetricsScope, MetricsSnapshot, NoopEventSink, OpEvent, OpKind, OpStage, SpanTimer,
    TerminalCategory, TerminalGuard, DEFAULT_EVENT_QUEUE_CAP, METRICS_SCHEMA_VERSION,
    OP_EVENT_SCHEMA_VERSION, PRIVACY_CANARY_MARKERS,
};
pub use output::{
    commit_text, format_result, write_result, write_result_to_path, CommitMode, OutputFormat,
    OutputTransaction, SymlinkPolicy, DEFAULT_MAX_OUTPUT_BYTES,
};
pub use partial::{PartialSession, PartialSessionConfig, PartialUpdate};
pub use pcm::PcmBuffer;
pub use postprocess::{normalize_result_with_report, NormalizationReport};
pub use product_contracts::{
    registered_stt_provider_ids, registered_tts_provider_ids, ProductContractsSnapshot,
    ProductProviderRecord, PRODUCT_CONTRACTS_SCHEMA_VERSION,
};
pub use profile::{
    format_recommendation, resolve_profile, ProfileResolution, QualityProfile,
    PROFILE_EVIDENCE_VERSION,
};
pub use provider_platform::{
    capabilities_for, check_builtin_conformance, list_provider_summaries,
    preflight_stt_with_registry, preflight_tts_with_registry, provider_list, ProviderBuildContext,
    ProviderId, ProviderList, ProviderRegistry, ProviderResolveOptions, ProviderSummary,
    PROVIDER_LIST_SCHEMA_VERSION,
};
pub use providers::local::{clear_context_cache, process_global_stt_pool, SttContextPool};
#[cfg(feature = "tts")]
pub use providers::{
    default_tts_model_for_provider, default_tts_voice_for_provider, lookup_openrouter_tts,
    resolve_tts_model, resolve_tts_voice, tts_model_known_for_provider, OpenRouterTtsProvider,
    DEFAULT_OPENROUTER_TTS_MODEL, DEFAULT_OPENROUTER_TTS_VOICE, OPENROUTER_TTS_REGISTRY,
};
#[cfg(feature = "tts")]
pub use providers::{
    lookup_elevenlabs_tts, ElevenLabsTtsProvider, DEFAULT_ELEVENLABS_TTS_MODEL,
    ELEVENLABS_TTS_REGISTRY, EXAMPLE_ELEVENLABS_VOICE_ID,
};
pub use providers::{
    lookup_openai_stt, OpenAiSttProvider, DEFAULT_OPENAI_STT_MODEL, OPENAI_STT_REGISTRY,
};
#[cfg(feature = "tts")]
pub use providers::{
    lookup_openai_tts, OpenAiTtsProvider, DEFAULT_OPENAI_TTS_MODEL, DEFAULT_OPENAI_TTS_VOICE,
    OPENAI_TTS_REGISTRY,
};
pub use providers::{lookup_xai_stt, XaiSttProvider, DEFAULT_XAI_STT_MODEL, XAI_STT_REGISTRY};
#[cfg(feature = "tts")]
pub use providers::{
    lookup_xai_tts, XaiTtsProvider, DEFAULT_XAI_TTS_MODEL, DEFAULT_XAI_TTS_VOICE, XAI_TTS_REGISTRY,
};
pub use providers::{
    LocalWhisperProvider, OpenRouterProvider, OpenRouterSttMode, Segment, TranscriptionOptions,
    TranscriptionProvider, TranscriptionResult,
};
pub use remote::{
    HardenedHttpClient, LongFormPolicy, OpenAiSpeechRequest, OpenRouterHttpPolicy,
    ProviderHttpPolicy, RemotePolicy, SpeechResponseFormat, TimestampSource,
    DEFAULT_REMOTE_STT_CHUNK_SECS,
};
pub use runtime::{
    GovernorConfig, Lifecycle, LifecycleState, OpContext, PermitKind, ResourceGovernor,
};
pub use sdk::{
    AurumConfig, CleanupConfig, OperationOptions, ProviderProfiles, RuntimeConfig, SttConfig,
    TranscriptionRequest,
};
#[cfg(feature = "tts")]
pub use sdk::{SynthesisRequest, TtsConfig};
pub use secret::SecretString;
pub use support::{
    build_support_bundle, default_bundle_path, SupportBundle, SUPPORT_BUNDLE_VERSION,
};
pub use window::{PartialClock, PartialWindowPolicy};

#[cfg(feature = "tts")]
pub use tts::{
    format_adapters as format_tts_adapters, format_custom_list as format_tts_custom_list,
    format_inspect as format_tts_inspect, format_model_list as format_tts_model_list,
    format_voice_list as format_tts_voice_list, inspect_pack as inspect_tts_pack,
    list_adapters as list_tts_adapters, list_models as list_tts_models,
    list_voices as list_tts_voices, process_global_tts_pool,
    propose_add_local as propose_tts_add_local, resolve_voice_for_model,
    run_kitten_catalogue_conformance, run_pack_conformance, verify_pack as verify_tts_pack,
    write_add_manifest as write_tts_add_manifest, write_wav_i16_mono_atomic,
    write_wav_i16_mono_transaction, BackendKind as TtsBackendKind, LocalTtsProvider,
    SynthesisOptions, SynthesisProvider, SynthesisResult, TrustMode, TtsSessionPool,
    DEFAULT_TTS_MODEL, DEFAULT_TTS_VOICE, KOKORO_DEFAULT_VOICE, KOKORO_TTS_MODEL,
};
