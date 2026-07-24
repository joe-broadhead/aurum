//! Local transcription via whisper.cpp (through the `whisper-rs` bindings).

use super::{Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult};
use crate::audio::AudioInput;
use crate::error::{ProviderError, Result};
use crate::model;
use async_trait::async_trait;
use std::path::PathBuf;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Local whisper.cpp provider.
///
/// Models are resolved from `cache_dir/models/` and downloaded on demand.
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

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        let model_name = options.model.clone();
        let language = options.language.clone();
        let timestamps = options.timestamps;
        let samples = input.samples.clone();
        let duration_secs = input.duration_secs;
        let cache_dir = self.cache_dir.clone();
        let show_progress = self.show_progress;

        // Ensure model on async side (download may need runtime).
        let model_path = model::ensure_model(&cache_dir, &model_name, show_progress).await?;

        // whisper-rs is synchronous and CPU/GPU heavy — run on a blocking thread.
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

        Ok(result)
    }
}

fn run_whisper(
    model_path: &std::path::Path,
    model_name: &str,
    samples: &[f32],
    duration_secs: f64,
    language: &str,
    timestamps: bool,
) -> Result<TranscriptionResult> {
    // Redirect whisper.cpp / ggml chatter away from stderr (no-op sinks unless
    // whisper-rs log/tracing backend features are enabled).
    whisper_rs::install_logging_hooks();

    let ctx_params = WhisperContextParameters::default();
    let ctx = WhisperContext::new_with_params(model_path.to_string_lossy().as_ref(), ctx_params)
        .map_err(|e| ProviderError::ModelLoad {
            model: model_name.to_string(),
            reason: e.to_string(),
        })?;

    let mut state = ctx
        .create_state()
        .map_err(|e| ProviderError::TranscriptionFailed {
            reason: format!("failed to create whisper state: {e}"),
        })?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    // Silence whisper.cpp's own stdout chatter; we own the UX.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_token_timestamps(timestamps);

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .clamp(1, 8);
    params.set_n_threads(n_threads);

    // Keep `lang` alive for the duration of `full()` — FullParams stores a raw pointer
    // into this string when a concrete language is set.
    //
    // NOTE: do NOT call `set_detect_language(true)`. On current whisper.cpp / whisper-rs
    // that flag returns zero segments even though language id is detected. Auto-detect is
    // achieved by passing language = None (or "auto") with detect_language left false.
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

    // Iterate segments via the high-level iterator API.
    for segment in state.as_iter() {
        // Timestamps from whisper.cpp are in centiseconds.
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

    // Best-effort language detection readout (returns -1 if unknown).
    let lang_id = state.full_lang_id_from_state();
    let detected = if lang_id >= 0 {
        whisper_rs::get_lang_str(lang_id).map(|s| s.to_string())
    } else {
        None
    };

    Ok(TranscriptionResult {
        text: full_text,
        segments,
        language: detected.or_else(|| {
            if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            }
        }),
        model: model_name.to_string(),
        provider: "local".to_string(),
        duration_secs,
    })
}
