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
use std::sync::Mutex;

/// RAII guard: sets engine `busy` and process-wide active-op count; clears on drop
/// (including panic unwinds through `block_on`).
struct BusyGuard<'a> {
    busy: &'a AtomicBool,
}

impl<'a> BusyGuard<'a> {
    fn acquire(busy: &'a AtomicBool, what: &str) -> Result<Self, FfiError> {
        if busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(FfiError::state(format!(
                "{what} already in progress on this engine (one exclusive op at a time)"
            )));
        }
        runtime::begin_op();
        Ok(Self { busy })
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::SeqCst);
        runtime::end_op();
    }
}

/// Local STT engine handle for embedders.
pub struct Engine {
    provider: LocalWhisperProvider,
    cancel: CancelFlag,
    /// True while preload or transcribe is in flight on this handle.
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

    /// Set last error from the C boundary (e.g. panic → INTERNAL).
    pub fn set_last_error_message(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.last_error.lock() {
            *g = msg.into();
        }
    }

    fn set_error(&self, msg: impl Into<String>) {
        self.set_last_error_message(msg);
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
    ///
    /// Exclusive with other ops on this engine (same busy flag as transcribe).
    pub fn preload(&self, model: &str) -> Result<(), FfiError> {
        let model = model.trim();
        if model.is_empty() {
            let e = FfiError::invalid_arg("model name is required");
            self.store_err(&e);
            return Err(e);
        }
        let _guard = match BusyGuard::acquire(&self.busy, "operation") {
            Ok(g) => g,
            Err(e) => {
                self.store_err(&e);
                return Err(e);
            }
        };

        let result = runtime::block_on(async { self.provider.preload(model).await });
        match result {
            Ok(Ok(_)) => {
                self.clear_error();
                Ok(())
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

    /// Request cooperative cancel of the in-flight transcription (if any).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Transcribe mono PCM at [`AURUM_SAMPLE_RATE`] Hz.
    ///
    /// At most one exclusive op (preload/transcribe) per engine at a time.
    /// Cancel flag is reset at start. On panic inside inference, busy is still
    /// released via [`BusyGuard`].
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

        let _guard = match BusyGuard::acquire(&self.busy, "transcription") {
            Ok(g) => g,
            Err(e) => {
                self.store_err(&e);
                return Err(e);
            }
        };

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

        // Borrow PCM through `block_on` — synchronous; no extra full-buffer copy.
        let outcome =
            runtime::block_on(async { self.provider.transcribe_pcm(samples, &options).await });

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

/// Process-level teardown: wait for in-flight ops, clear whisper cache, stop new work.
///
/// Hosts must not start new calls after this. Safe to call with no engines left.
/// Prefer destroy engines first; then `shutdown` before process exit (Metal).
pub fn shutdown() {
    runtime::shutdown_runtime();
    // Only clear contexts once ops have drained (shutdown_runtime waits briefly).
    if runtime::active_ops() == 0 {
        clear_context_cache();
    } else {
        // Still clear — process is exiting; better than leaking Metal state.
        clear_context_cache();
    }
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

    #[test]
    fn busy_guard_releases_on_error_path() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();
        // First preload fails (not cached) but must release busy.
        let _ = engine.preload("tiny-q5_1");
        // Second exclusive op must not get permanent STATE from leaked busy.
        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::ModelNotReady);
        assert!(!engine.busy.load(Ordering::SeqCst));
    }

    #[test]
    fn exclusive_op_rejects_while_busy() {
        let dir = tempdir().unwrap();
        let engine = Engine::new(EngineConfig {
            cache_dir: dir.path().display().to_string(),
            local_only: true,
            progress_logging: false,
        })
        .unwrap();

        let _g = BusyGuard::acquire(&engine.busy, "test").unwrap();
        let err = engine
            .transcribe_pcm(
                &[0.0; 100],
                &TranscribeOpts {
                    model: "tiny-q5_1".into(),
                    language: "en".into(),
                    timestamps: false,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, FfiStatus::State);

        let err = engine.preload("tiny-q5_1").unwrap_err();
        assert_eq!(err.status, FfiStatus::State);
    }
}
