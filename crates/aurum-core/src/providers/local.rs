//! Local transcription via whisper.cpp (through the `whisper-rs` bindings).
//!
//! Maintains a process-level cache of loaded `WhisperContext` values keyed by
//! model path so repeated calls (library batch use, tests) do not reload ggml.
//!
//! Concurrent cold starts coalesce via singleflight (JOE-1597). Resident weight
//! and eviction use the shared model registry (JOE-1598). Inference threads and
//! blocking work are admitted by the process [`ResourceGovernor`] (JOE-1596/1599).
//!
//! Embedders (mic hosts) should prefer:
//! - [`LocalWhisperProvider::preload`] at startup
//! - [`LocalWhisperProvider::transcribe_pcm`] or [`AudioInput::from_pcm`] for buffers
//! - [`crate::pcm::PcmBuffer`] to accumulate capture chunks

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::{AudioInput, WHISPER_SAMPLE_RATE};
use crate::error::{ProviderError, Result};
use crate::model::{self, DownloadProgressCallback, EnsureModelOptions};
use crate::postprocess;
use crate::runtime::{
    LoadKey, ModelRegistry, OpContext, PermitKind, RegistryConfig, RegistryPin, ResidencyWeight,
    ResourceGovernor, Singleflight,
};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static LOGGING_HOOKS: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();

/// Process-global STT context residency + singleflight loader.
struct ContextCache {
    flight: Singleflight<WhisperContext>,
    registry: ModelRegistry<WhisperContext>,
}

impl ContextCache {
    fn new() -> Self {
        Self {
            flight: Singleflight::default(),
            registry: ModelRegistry::new(RegistryConfig::default()),
        }
    }

    fn load_key(model_path: &Path, model_name: &str) -> LoadKey {
        LoadKey::stt(model_name, model_path.display().to_string())
    }

    /// Conservative weight: on-disk size + 50% runtime overhead headroom.
    fn weight_for(model_path: &Path) -> ResidencyWeight {
        let disk = std::fs::metadata(model_path).map(|m| m.len()).unwrap_or(0);
        let bytes = disk.saturating_add(disk / 2).max(16 * 1024 * 1024);
        ResidencyWeight { bytes }
    }

    /// Load or reuse a context and return an **active registry pin** held for the
    /// whole operation (JOE-1646). No unpinned Arc is returned to callers.
    fn get_or_load_pin(
        &self,
        model_path: &Path,
        model_name: &str,
    ) -> Result<RegistryPin<WhisperContext>> {
        let key = Self::load_key(model_path, model_name);
        // Atomic lookup+pin under registry lock.
        if let Some(pin) = self.registry.get_and_pin(&key) {
            return Ok(pin);
        }

        let weight = Self::weight_for(model_path);
        if weight.bytes > self.registry.config().max_resident_bytes {
            return Err(ProviderError::Overload {
                reason: format!(
                    "model weight {} exceeds residency budget {}",
                    weight.bytes,
                    self.registry.config().max_resident_bytes
                ),
            }
            .into());
        }

        // Coalesce concurrent cold loads; LeaderGuard is panic-safe.
        loop {
            if let Some(pin) = self.registry.get_and_pin(&key) {
                return Ok(pin);
            }
            match self.flight.begin_or_wait_guard(key.clone()) {
                Ok(None) => {
                    // Leader finished — re-check registry.
                    continue;
                }
                Err(message) => {
                    return Err(ProviderError::ModelLoad {
                        model: model_name.to_string(),
                        reason: message,
                    }
                    .into());
                }
                Ok(Some(leader)) => {
                    let gov = ResourceGovernor::process_global();
                    let path_owned = model_path.to_path_buf();
                    let name_owned = model_name.to_string();

                    let load_result = (|| -> Result<Arc<WhisperContext>> {
                        let _permit = gov.acquire(PermitKind::ModelLoad, None)?;
                        LOGGING_HOOKS.get_or_init(|| {
                            whisper_rs::install_logging_hooks();
                        });
                        let params = WhisperContextParameters::default();
                        let ctx = WhisperContext::new_with_params(
                            path_owned.to_string_lossy().as_ref(),
                            params,
                        )
                        .map_err(|e| ProviderError::ModelLoad {
                            model: name_owned.clone(),
                            reason: e.to_string(),
                        })?;
                        Ok(Arc::new(ctx))
                    })();

                    match load_result {
                        Ok(ctx) => {
                            match self.registry.insert_and_pin(
                                key.clone(),
                                Arc::clone(&ctx),
                                weight,
                            ) {
                                Ok(pin) => {
                                    leader.success();
                                    return Ok(pin);
                                }
                                Err(e) => {
                                    leader.fail(e.to_string());
                                    drop(ctx);
                                    return Err(e);
                                }
                            }
                        }
                        Err(e) => {
                            leader.fail(e.to_string());
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    fn get_or_load(&self, model_path: &Path, model_name: &str) -> Result<Arc<WhisperContext>> {
        Ok(Arc::clone(
            self.get_or_load_pin(model_path, model_name)?.value(),
        ))
    }

    fn clear(&self) {
        self.registry.clear();
        self.flight.clear();
    }

    fn contains(&self, model_path: &Path, model_name: &str) -> bool {
        let key = Self::load_key(model_path, model_name);
        self.registry.get(&key).is_some()
    }

    /// Snapshot residency diagnostics (registry only — no hidden flight Ready).
    #[allow(dead_code)]
    fn residency_len(&self) -> usize {
        self.registry.len()
    }
}

static CONTEXT_CACHE: Lazy<ContextCache> = Lazy::new(ContextCache::new);

/// Drop all cached whisper contexts (call before process exit, only when idle).
pub fn clear_context_cache() {
    CONTEXT_CACHE.clear();
}

/// Local whisper.cpp provider.
pub struct LocalWhisperProvider {
    cache_dir: PathBuf,
    show_progress: bool,
    /// When true, never download models — fail if missing from cache.
    local_only: bool,
    on_download_progress: Option<DownloadProgressCallback>,
}

impl LocalWhisperProvider {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            show_progress: true,
            local_only: false,
            on_download_progress: None,
        }
    }

    pub fn with_progress(mut self, show: bool) -> Self {
        self.show_progress = show;
        self
    }

    /// Fail closed if the model is not already on disk (no network).
    pub fn with_local_only(mut self, local_only: bool) -> Self {
        self.local_only = local_only;
        self
    }

    /// Structured download progress for UI hosts (in addition to optional CLI bar).
    pub fn with_download_progress(mut self, cb: DownloadProgressCallback) -> Self {
        self.on_download_progress = Some(cb);
        self
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn local_only(&self) -> bool {
        self.local_only
    }

    /// Whether the ggml file is present and passes basic integrity checks.
    pub fn is_model_cached(&self, model: &str) -> bool {
        model::is_model_cached(&self.cache_dir, model)
    }

    /// Whether a WhisperContext for this model is already loaded in-process.
    pub fn is_model_loaded(&self, model: &str) -> bool {
        let Ok(info) = model::lookup_model(model) else {
            return false;
        };
        let path = model::model_path(&self.cache_dir, info);
        CONTEXT_CACHE.contains(&path, model)
    }

    fn ensure_opts(&self) -> EnsureModelOptions {
        let mut opts = EnsureModelOptions::new()
            .local_only(self.local_only)
            .show_progress(self.show_progress);
        if let Some(cb) = &self.on_download_progress {
            opts = opts.on_progress(Arc::clone(cb));
        }
        opts
    }

    /// Download (unless `local_only`) and load the model into the process cache.
    pub async fn preload(&self, model: &str) -> Result<PathBuf> {
        let path =
            model::ensure_model_with_options(&self.cache_dir, model, self.ensure_opts()).await?;
        let model_name = model.to_string();
        let path_clone = path.clone();
        tokio::task::spawn_blocking(move || CONTEXT_CACHE.get_or_load(&path_clone, &model_name))
            .await
            .map_err(|e| ProviderError::TranscriptionFailed {
                reason: format!("preload worker panicked: {e}"),
            })??;
        Ok(path)
    }

    /// Transcribe a mono PCM buffer (must be [`WHISPER_SAMPLE_RATE`] Hz).
    ///
    /// Preferred entry point for mic hosts — no temp files, no ffmpeg.
    pub async fn transcribe_pcm(
        &self,
        samples: &[f32],
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        let input = AudioInput::from_pcm_slice(samples, WHISPER_SAMPLE_RATE)?;
        self.transcribe(&input, options).await
    }

    /// Transcribe an owned/shared PCM buffer already wrapped as [`AudioInput`].
    pub async fn transcribe_audio(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        self.transcribe(input, options).await
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Asr
    }

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        if input.sample_rate != WHISPER_SAMPLE_RATE {
            return Err(crate::error::UserError::UnsupportedSampleRate {
                got: input.sample_rate,
                need: WHISPER_SAMPLE_RATE,
            }
            .into());
        }

        let op = OpContext::from_optional_cancel(options.cancel.clone());
        op.check()?;
        op.emit("stt", "ensure_model");

        let model_name = options.model.clone();
        let language = options.language.clone();
        let timestamps = options.timestamps;
        let cancel = op.cancel.clone();
        let samples: Arc<[f32]> = Arc::clone(&input.samples);
        let duration_secs = input.duration_secs;
        let cache_dir = self.cache_dir.clone();
        let opts = self.ensure_opts();

        let model_path = model::ensure_model_with_options(&cache_dir, &model_name, opts).await?;

        op.check()?;
        op.emit("stt", "inference");

        let result = tokio::task::spawn_blocking(move || {
            // Admission inside the worker so queue wait does not occupy a Tokio worker.
            let gov = ResourceGovernor::process_global();
            let n_threads = gov.recommend_stt_threads();
            let op_inner = OpContext::with_cancel(cancel.clone());
            let _permit = gov.acquire_stt(n_threads, 0, Some(&op_inner))?;
            // Re-check after queue wait so late work does not start after cancel.
            op_inner.check()?;
            run_whisper(
                &model_path,
                &model_name,
                &samples,
                duration_secs,
                &language,
                timestamps,
                Some(&cancel),
                n_threads as i32,
            )
        })
        .await
        .map_err(|e| ProviderError::TranscriptionFailed {
            reason: format!("worker thread panicked: {e}"),
        })??;

        Ok(postprocess::normalize_result(result))
    }
}

unsafe extern "C" fn abort_callback_atomic(user_data: *mut std::ffi::c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    let flag = &*(user_data as *const std::sync::atomic::AtomicBool);
    flag.load(std::sync::atomic::Ordering::SeqCst)
}

#[allow(clippy::too_many_arguments)]
fn run_whisper(
    model_path: &Path,
    model_name: &str,
    samples: &[f32],
    duration_secs: f64,
    language: &str,
    timestamps: bool,
    cancel: Option<&crate::cancel::CancelFlag>,
    n_threads: i32,
) -> Result<TranscriptionResult> {
    if cancel.is_some_and(|c| c.is_cancelled()) {
        return Err(ProviderError::Cancelled.into());
    }

    // Atomic registry lease held for the entire decode (JOE-1646).
    let lease = CONTEXT_CACHE.get_or_load_pin(model_path, model_name)?;
    let ctx = Arc::clone(lease.value());

    let mut state = ctx
        .create_state()
        .map_err(|e| ProviderError::TranscriptionFailed {
            reason: format!("failed to create whisper state: {e}"),
        })?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_token_timestamps(timestamps);
    params.set_no_speech_thold(0.6);

    // Keep Arc alive across full() so the C abort callback's pointer stays valid.
    let _cancel_keep: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> =
        cancel.map(|f| f.clone_arc());
    if let Some(ref arc) = _cancel_keep {
        let ptr = std::sync::Arc::as_ptr(arc) as *mut std::ffi::c_void;
        unsafe {
            params.set_abort_callback(Some(abort_callback_atomic));
            params.set_abort_callback_user_data(ptr);
        }
    }

    params.set_n_threads(n_threads.clamp(1, 8));

    let lang = language.trim().to_ascii_lowercase();
    let auto = lang.is_empty() || lang == "auto";
    if auto {
        params.set_language(None);
    } else {
        params.set_language(Some(lang.as_str()));
    }

    let full_res = state.full(params, samples);
    if cancel.is_some_and(|c| c.is_cancelled()) {
        return Err(ProviderError::Cancelled.into());
    }
    full_res.map_err(|e| ProviderError::TranscriptionFailed {
        reason: e.to_string(),
    })?;

    let mut segments = Vec::new();
    let mut full_text = String::new();

    for segment in state.as_iter() {
        let start = segment.start_timestamp() as f64 / 100.0;
        let end = segment.end_timestamp() as f64 / 100.0;
        let text = segment.to_string();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if !full_text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(text);
        segments.push(Segment {
            start,
            end,
            text: text.to_string(),
        });
    }

    let lang_id = state.full_lang_id_from_state();
    let detected = if lang_id >= 0 {
        whisper_rs::get_lang_str(lang_id).map(|s| s.to_string())
    } else {
        None
    };

    let result = TranscriptionResult::local(
        full_text,
        segments,
        detected.or_else(|| {
            if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            }
        }),
        model_name.to_string(),
        duration_secs,
    );
    // Explicitly hold the registry lease until after decode completes.
    drop(lease);
    Ok(result)
}
