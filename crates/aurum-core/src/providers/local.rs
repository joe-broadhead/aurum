//! Local transcription via whisper.cpp (through the `whisper-rs` bindings).
//!
//! Maintains a process-level cache of loaded `WhisperContext` values keyed by
//! model path so repeated calls (library batch use, tests) do not reload ggml.

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::AudioInput;
use crate::error::{ProviderError, Result};
use crate::model;
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
        // Another thread may have inserted while we loaded — prefer existing.
        let entry = guard.entry(key).or_insert_with(|| Arc::clone(&ctx));
        Ok(Arc::clone(entry))
    }

    /// Drop all cached contexts now, while Metal/GPU is still valid.
    ///
    /// Must be called before process exit. If contexts are still alive when the
    /// Metal device singleton is torn down (static destructors), ggml asserts.
    fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.clear();
        }
    }
}

impl Drop for ContextCache {
    fn drop(&mut self) {
        // Best-effort: if the CLI forgot to clear, try dropping here. On some
        // platforms Metal may already be gone; prefer explicit clear in main.
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
}

impl LocalWhisperProvider {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            show_progress: true,
        }
    }

    pub fn with_progress(mut self, show: bool) -> Self {
        self.show_progress = show;
        self
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
        let model_name = options.model.clone();
        let language = options.language.clone();
        let timestamps = options.timestamps;
        let samples: Arc<[f32]> = Arc::clone(&input.samples);
        let duration_secs = input.duration_secs;
        let cache_dir = self.cache_dir.clone();
        let show_progress = self.show_progress;

        let model_path = model::ensure_model(&cache_dir, &model_name, show_progress).await?;

        let result = tokio::task::spawn_blocking(move || {
            run_whisper(
                &model_path,
                &model_name,
                &samples,
                duration_secs,
                &language,
                timestamps,
            )
        })
        .await
        .map_err(|e| ProviderError::TranscriptionFailed {
            reason: format!("worker thread panicked: {e}"),
        })??;

        Ok(postprocess::normalize_result(result))
    }
}

fn run_whisper(
    model_path: &Path,
    model_name: &str,
    samples: &[f32],
    duration_secs: f64,
    language: &str,
    timestamps: bool,
) -> Result<TranscriptionResult> {
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

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .clamp(1, 8);
    params.set_n_threads(n_threads);

    // Keep `lang` alive for the duration of `full()`.
    // Do NOT call set_detect_language(true) — empty segments on current whisper-rs.
    let lang = language.trim().to_ascii_lowercase();
    let auto = lang.is_empty() || lang == "auto";
    if auto {
        params.set_language(None);
    } else {
        params.set_language(Some(lang.as_str()));
    }

    state
        .full(params, samples)
        .map_err(|e| ProviderError::TranscriptionFailed {
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
