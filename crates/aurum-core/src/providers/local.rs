//! Local transcription via whisper.cpp (through the `whisper-rs` bindings).
//!
//! Maintains a process-level cache of loaded `WhisperContext` values keyed by
//! model path so repeated calls (library batch use, tests) do not reload ggml.
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
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static LOGGING_HOOKS: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();

/// Process-global cache of loaded whisper contexts.
static CONTEXT_CACHE: Lazy<ContextCache> = Lazy::new(ContextCache::new);

struct ContextCache {
    inner: Mutex<HashMap<PathBuf, Arc<WhisperContext>>>,
}

impl ContextCache {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn get_or_load(&self, model_path: &Path, model_name: &str) -> Result<Arc<WhisperContext>> {
        let key = model_path.to_path_buf();
        {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ctx) = guard.get(&key) {
                tracing::debug!(path = %key.display(), "reusing cached whisper context");
                return Ok(Arc::clone(ctx));
            }
        }

        LOGGING_HOOKS.get_or_init(|| {
            whisper_rs::install_logging_hooks();
        });

        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(model_path.to_string_lossy().as_ref(), params)
            .map_err(|e| ProviderError::ModelLoad {
                model: model_name.to_string(),
                reason: e.to_string(),
            })?;
        let ctx = Arc::new(ctx);

        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(key).or_insert_with(|| Arc::clone(&ctx));
        Ok(Arc::clone(entry))
    }

    fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.clear();
        }
    }

    fn contains(&self, model_path: &Path) -> bool {
        self.inner
            .lock()
            .map(|g| g.contains_key(model_path))
            .unwrap_or(false)
    }
}

impl Drop for ContextCache {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Drop all cached whisper contexts (call before process exit).
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
        CONTEXT_CACHE.contains(&path)
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

        if options.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(ProviderError::Cancelled.into());
        }

        let model_name = options.model.clone();
        let language = options.language.clone();
        let timestamps = options.timestamps;
        let cancel = options.cancel.clone();
        let samples: Arc<[f32]> = Arc::clone(&input.samples);
        let duration_secs = input.duration_secs;
        let cache_dir = self.cache_dir.clone();
        let opts = self.ensure_opts();

        let model_path = model::ensure_model_with_options(&cache_dir, &model_name, opts).await?;

        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(ProviderError::Cancelled.into());
        }

        let result = tokio::task::spawn_blocking(move || {
            run_whisper(
                &model_path,
                &model_name,
                &samples,
                duration_secs,
                &language,
                timestamps,
                cancel.as_ref(),
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

fn run_whisper(
    model_path: &Path,
    model_name: &str,
    samples: &[f32],
    duration_secs: f64,
    language: &str,
    timestamps: bool,
    cancel: Option<&crate::cancel::CancelFlag>,
) -> Result<TranscriptionResult> {
    if cancel.is_some_and(|c| c.is_cancelled()) {
        return Err(ProviderError::Cancelled.into());
    }

    let ctx = CONTEXT_CACHE.get_or_load(model_path, model_name)?;

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

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .clamp(1, 8);
    params.set_n_threads(n_threads);

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

    Ok(TranscriptionResult::local(
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
    ))
}
