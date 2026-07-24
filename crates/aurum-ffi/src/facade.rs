//! Pure Rust façade — single source of behavior for C (and future UniFFI).

use crate::error::{FfiError, FfiStatus};
use crate::runtime;
use crate::types::{
    CleanupStyle, EngineConfig, Segment, TranscribeOpts, Transcript, AURUM_SAMPLE_RATE,
};
use aurum_core::cancel::CancelFlag;
use aurum_core::cleanup::{cleanup_text, RulesCleanup, TextCleanup};
use aurum_core::providers::local::clear_context_cache;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Local STT engine handle for embedders.
pub struct Engine {
    provider: LocalWhisperProvider,
    cancel: CancelFlag,
    /// True while a transcribe_pcm call is in flight on this handle.
    busy: AtomicBool,
    last_error: Mutex<String>,
}

impl Engine {
    /// Create an engine. `cache_dir` must be non-empty.
    pub fn new(config: EngineConfig) -> Result<Self, FfiError> {
        if config.cache_dir.trim().is_empty() {
            return Err(FfiError::invalid_arg(
                "cache_dir is required (host must supply a writable model cache path)",
            ));
        }
        let provider = LocalWhisperProvider::new(PathBuf::from(config.cache_dir.trim()))
            .with_progress(config.progress_logging)
            .with_local_only(config.local_only);
        Ok(Self {
            provider,
            cancel: CancelFlag::new(),
            busy: AtomicBool::new(false),
            last_error: Mutex::new(String::new()),
        })
    }

    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn set_error(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.last_error.lock() {
            *g = msg.into();
        }
    }

    fn clear_error(&self) {
        if let Ok(mut g) = self.last_error.lock() {
            g.clear();
        }
    }

    fn store_err(&self, err: &FfiError) {
        self.set_error(err.message.clone());
    }

    /// Whether the ggml file is present for `model`.
    pub fn is_model_ready(&self, model: &str) -> bool {
        if model.trim().is_empty() {
            return false;
        }
        self.provider.is_model_cached(model.trim())
    }

    /// Ensure model on disk (unless local_only) and warm the process context cache.
    pub fn preload(&self, model: &str) -> Result<(), FfiError> {
        let model = model.trim();
        if model.is_empty() {
            let e = FfiError::invalid_arg("model name is required");
            self.store_err(&e);
            return Err(e);
        }
        if self.busy.load(Ordering::SeqCst) {
            let e = FfiError::state("cannot preload while transcription is in progress");
            self.store_err(&e);
            return Err(e);
        }
        let result = runtime::block_on(async { self.provider.preload(model).await });
        match result {
            Ok(inner) => match inner {
                Ok(_) => {
                    self.clear_error();
                    Ok(())
                }
                Err(e) => {
                    let fe = FfiError::from(e);
                    self.store_err(&fe);
                    Err(fe)
                }
            },
            Err(e) => {
                self.store_err(&e);
                Err(e)
            }
        }
    }

    /// Request cooperative cancel of the in-flight transcription (if any).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Transcribe mono PCM at [`AURUM_SAMPLE_RATE`] Hz.
    ///
    /// At most one call per engine at a time. Cancel flag is reset at start.
    pub fn transcribe_pcm(
        &self,
        samples: &[f32],
        opts: &TranscribeOpts,
    ) -> Result<Transcript, FfiError> {
        let model = opts.model.trim();
        if model.is_empty() {
            let e = FfiError::invalid_arg("model name is required");
            self.store_err(&e);
            return Err(e);
        }
        if samples.is_empty() {
            let e = FfiError::new(FfiStatus::Audio, "PCM buffer is empty");
            self.store_err(&e);
            return Err(e);
        }
        if samples.iter().any(|s| !s.is_finite()) {
            let e = FfiError::new(FfiStatus::Audio, "PCM contains NaN or Inf samples");
            self.store_err(&e);
            return Err(e);
        }

        // Acquire busy lock.
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            let e = FfiError::state(
                "transcription already in progress on this engine (one call at a time)",
            );
            self.store_err(&e);
            return Err(e);
        }

        self.cancel.reset();
        let language = if opts.language.trim().is_empty() {
            "auto".to_string()
        } else {
            opts.language.trim().to_string()
        };
        let options = TranscriptionOptions {
            model: model.to_string(),
            language,
            timestamps: opts.timestamps,
            cancel: Some(self.cancel.clone()),
        };

        // Copy samples so the async worker does not borrow host memory across await
        // in a way that confuses lifetimes if we later spawn; slice is Sync enough for block_on.
        let pcm = samples.to_vec();
        let provider = &self.provider;

        let outcome =
            runtime::block_on(async move { provider.transcribe_pcm(&pcm, &options).await });

        self.busy.store(false, Ordering::SeqCst);

        match outcome {
            Ok(Ok(result)) => {
                self.clear_error();
                Ok(Transcript {
                    text: result.text,
                    language: result.language,
                    model: result.model,
                    duration_secs: result.duration_secs,
                    timestamps_reliable: result.timestamps_reliable,
                    segments: result
                        .segments
                        .into_iter()
                        .map(|s| Segment {
                            start_s: s.start,
                            end_s: s.end,
                            text: s.text,
                        })
                        .collect(),
                    cleanup_style: CleanupStyle::Raw,
                })
            }
            Ok(Err(e)) => {
                let fe = FfiError::from(e);
                self.store_err(&fe);
                Err(fe)
            }
            Err(e) => {
                self.store_err(&e);
                Err(e)
            }
        }
    }

    /// Sample rate hosts must use (Hz).
    pub fn sample_rate(&self) -> u32 {
        AURUM_SAMPLE_RATE
    }

    #[allow(dead_code)]
    fn _lock_error(&self) -> Option<MutexGuard<'_, String>> {
        self.last_error.lock().ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Best-effort: do not clear global cache here (other engines may exist).
        // Hosts should call `shutdown()` on process exit.
    }
}

/// On-device rules cleanup (no network, no engine handle required).
pub fn cleanup_rules(text: &str, style: CleanupStyle) -> Result<String, FfiError> {
    let rules = RulesCleanup::new();
    let core_style = style.to_core();
    let outcome = runtime::block_on(async {
        cleanup_text(text, &rules as &dyn TextCleanup, core_style).await
    })?;
    match outcome {
        Ok(r) => Ok(r.text),
        Err(e) => Err(FfiError::from(e)),
    }
}

/// Process-level teardown: drop Tokio runtime and clear whisper context cache (Metal-safe).
pub fn shutdown() {
    clear_context_cache();
    runtime::shutdown_runtime();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_empty_cache_dir() {
        match Engine::new(EngineConfig {
            cache_dir: "  ".into(),
            local_only: true,
            progress_logging: false,
        }) {
            Ok(_) => panic!("expected error"),
            Err(err) => assert_eq!(err.status, FfiStatus::InvalidArg),
        }
    }

    #[test]
    fn cleanup_rules_strips_fillers() {
        let out = cleanup_rules("um, hello there", CleanupStyle::Clean).unwrap();
        assert!(out.to_ascii_lowercase().contains("hello"));
        assert!(!out.to_ascii_lowercase().contains("um"));
    }

    #[test]
    fn cleanup_raw_trims() {
        let out = cleanup_rules("  hi  ", CleanupStyle::Raw).unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn empty_pcm_errors() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let err = engine
            .transcribe_pcm(
                &[],
                &TranscribeOpts {
                    model: "tiny-q5_1".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::Audio);
    }

    #[test]
    fn missing_model_local_only() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        assert!(!engine.is_model_ready("tiny-q5_1"));
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
    }

    #[test]
    fn requires_model_name() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        let err = engine
            .transcribe_pcm(
                &[0.0; 100],
                &TranscribeOpts {
                    model: "".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::InvalidArg);
    }
}
