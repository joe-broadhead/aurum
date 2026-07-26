//! Local ONNX KittenTTS provider (MIT binary path — no GPL phonemizer).

use super::catalogue::{ensure_voice_pack, lookup_model, lookup_voice, onnx_path, voices_path};
use super::npz::load_voices_npz;
use super::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use super::tokenize::ipa_to_ids;
use super::validate::{clamp_speaking_rate, prepare_text, DEFAULT_MAX_CHARS};
use super::wav::peak_guard_f32_to_i16;
use crate::error::{ProviderError, Result, UserError};
use async_trait::async_trait;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Samples trimmed from the tail of every chunk (trailing silence artifact).
const TAIL_TRIM: usize = 2_000;
/// Peak limit before i16 quantization.
const PEAK_LIMIT: f32 = 0.95;

/// On-device KittenTTS via ONNX Runtime + misaki-rs G2P (no espeak / GPL).
pub struct LocalTtsProvider {
    cache_dir: PathBuf,
    show_progress: bool,
    local_only: bool,
    max_chars: usize,
    /// Lazily loaded sessions keyed by model id.
    sessions: Mutex<HashMap<String, Arc<LoadedPack>>>,
}

struct LoadedPack {
    session: Mutex<Session>,
    voices: HashMap<String, super::npz::VoiceMatrix>,
    sample_rate_hz: u32,
    /// Optional speed priors from config.json (internal key → multiplier).
    speed_priors: HashMap<String, f32>,
}

impl LocalTtsProvider {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            show_progress: false,
            local_only: false,
            max_chars: DEFAULT_MAX_CHARS,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_progress(mut self, v: bool) -> Self {
        self.show_progress = v;
        self
    }

    pub fn with_local_only(mut self, v: bool) -> Self {
        self.local_only = v;
        self
    }

    pub fn with_max_chars(mut self, n: usize) -> Self {
        self.max_chars = n.max(1);
        self
    }

    async fn ensure_loaded(&self, model: &str, local_only: bool) -> Result<Arc<LoadedPack>> {
        {
            let guard = self.sessions.lock().map_err(|_| {
                crate::error::TranscriptionError::internal("TTS session map poisoned")
            })?;
            if let Some(pack) = guard.get(model) {
                return Ok(Arc::clone(pack));
            }
        }

        let info = lookup_model(model)?;
        let _pack_dir = ensure_voice_pack(
            &self.cache_dir,
            model,
            self.show_progress,
            local_only || self.local_only,
        )
        .await?;

        let onnx = onnx_path(&self.cache_dir, info);
        let voices_file = voices_path(&self.cache_dir, info);
        let speed_priors = load_speed_priors(&self.cache_dir, info);
        let sample_rate = info.sample_rate_hz;

        let loaded = tokio::task::spawn_blocking(move || {
            load_pack(&onnx, &voices_file, sample_rate, speed_priors)
        })
        .await
        .map_err(|e| crate::error::TranscriptionError::internal(format!("TTS load join: {e}")))??;

        let arc = Arc::new(loaded);
        let mut guard = self
            .sessions
            .lock()
            .map_err(|_| crate::error::TranscriptionError::internal("TTS session map poisoned"))?;
        let entry = guard
            .entry(model.to_string())
            .or_insert_with(|| Arc::clone(&arc));
        Ok(Arc::clone(entry))
    }
}

fn synthesize_with_pack(
    pack: &LoadedPack,
    text: &str,
    opts: &SynthesisOptions,
    text_chars: usize,
    text_truncated: bool,
) -> Result<SynthesisResult> {
    if let Some(flag) = &opts.cancel {
        if flag.is_cancelled() {
            return Err(ProviderError::Cancelled.into());
        }
    }

    let voice = lookup_voice(&opts.voice)?;
    let internal = voice.internal_key;
    let rate = clamp_speaking_rate(opts.speaking_rate);

    // G2P (MIT misaki-rs, no espeak).
    let g2p = misaki_rs::G2P::new(misaki_rs::Language::EnglishUS);
    let (ipa, _) = g2p.g2p(text).map_err(|e| ProviderError::Other {
        message: format!("G2P failed: {e}"),
    })?;
    // Strip unknown markers that misaki may emit without espeak fallback.
    let ipa = ipa.replace('❓', "");
    if ipa.trim().is_empty() {
        return Err(ProviderError::Other {
            message: "G2P produced empty phonemes for input text".into(),
        }
        .into());
    }

    let voice_mat = pack
        .voices
        .get(internal)
        .or_else(|| pack.voices.get(&opts.voice))
        .ok_or_else(|| UserError::Other {
            message: format!(
                "voice embedding '{internal}' missing from pack; available: {:?}",
                pack.voices.keys().collect::<Vec<_>>()
            ),
        })?;

    let effective_speed = rate * pack.speed_priors.get(internal).copied().unwrap_or(1.0);
    let ids = ipa_to_ids(&ipa);
    if ids.len() <= 2 {
        return Err(ProviderError::Other {
            message: format!("no tokenizable phonemes from IPA {ipa:?}"),
        }
        .into());
    }
    let seq_len = ids.len();
    let style = voice_mat.style_row(text.chars().count()).to_vec();
    let style_dim = style.len();

    let t_ids =
        Tensor::<i64>::from_array(([1usize, seq_len], ids)).map_err(|e| ProviderError::Other {
            message: format!("input_ids tensor: {e}"),
        })?;
    let t_style = Tensor::<f32>::from_array(([1usize, style_dim], style)).map_err(|e| {
        ProviderError::Other {
            message: format!("style tensor: {e}"),
        }
    })?;
    let t_speed = Tensor::<f32>::from_array(([1usize], vec![effective_speed])).map_err(|e| {
        ProviderError::Other {
            message: format!("speed tensor: {e}"),
        }
    })?;

    let mut session = pack
        .session
        .lock()
        .map_err(|_| crate::error::TranscriptionError::internal("ORT session mutex poisoned"))?;
    let outputs = session
        .run(ort::inputs![t_ids, t_style, t_speed])
        .map_err(|e| ProviderError::Other {
            message: format!("ONNX inference failed: {e}"),
        })?;
    let (_shape, audio_data) =
        outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Other {
                message: format!("extract audio tensor: {e}"),
            })?;
    let audio_flat: Vec<f32> = audio_data.to_vec();
    let trimmed_len = audio_flat.len().saturating_sub(TAIL_TRIM);
    let audio = &audio_flat[..trimmed_len];
    if audio.is_empty() {
        return Err(ProviderError::Other {
            message: "synthesis produced empty audio".into(),
        }
        .into());
    }

    let pcm = peak_guard_f32_to_i16(audio, PEAK_LIMIT);
    let sample_rate = opts.sample_rate_hz.unwrap_or(pack.sample_rate_hz);
    let duration_ms = (pcm.len() as u64)
        .saturating_mul(1000)
        .checked_div(sample_rate as u64)
        .unwrap_or(0);

    Ok(SynthesisResult {
        pcm_i16_mono: pcm,
        sample_rate_hz: sample_rate,
        channels: 1,
        backend_kind: BackendKind::Local,
        provider: "local".into(),
        model: opts.model.clone(),
        voice: voice.id.to_string(),
        language: opts.language.clone(),
        duration_ms,
        text_chars,
        text_truncated,
    })
}

fn load_pack(
    onnx: &Path,
    voices_file: &Path,
    sample_rate_hz: u32,
    speed_priors: HashMap<String, f32>,
) -> Result<LoadedPack> {
    let session = Session::builder()
        .map_err(|e| ProviderError::ModelLoad {
            model: onnx.display().to_string(),
            reason: format!("ORT session builder: {e}"),
        })?
        .commit_from_file(onnx)
        .map_err(|e| ProviderError::ModelLoad {
            model: onnx.display().to_string(),
            reason: format!("load ONNX: {e}"),
        })?;
    let voices = load_voices_npz(voices_file)?;
    Ok(LoadedPack {
        session: Mutex::new(session),
        voices,
        sample_rate_hz,
        speed_priors,
    })
}

fn load_speed_priors(
    cache_dir: &Path,
    info: &super::catalogue::TtsModelInfo,
) -> HashMap<String, f32> {
    let path = super::catalogue::config_path(cache_dir, info);
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    if let Some(map) = v.get("speed_priors").and_then(|x| x.as_object()) {
        for (k, val) in map {
            if let Some(f) = val.as_f64() {
                out.insert(k.clone(), f as f32);
            }
        }
    }
    out
}

#[async_trait]
impl SynthesisProvider for LocalTtsProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult> {
        let prepared = prepare_text(text, self.max_chars)?;
        lookup_model(&opts.model)?;
        lookup_voice(&opts.voice)?;

        let local_only = opts.local_only || self.local_only;
        let pack = self.ensure_loaded(&opts.model, local_only).await?;

        if let Some(flag) = &opts.cancel {
            if flag.is_cancelled() {
                return Err(ProviderError::Cancelled.into());
            }
        }

        let timeout = Duration::from_millis(if opts.timeout_ms == 0 {
            super::validate::DEFAULT_TIMEOUT_MS
        } else {
            opts.timeout_ms
        });

        let text_owned = prepared.text.clone();
        let text_chars = prepared.text_chars;
        let text_truncated = prepared.text_truncated;
        let opts_owned = opts.clone();
        let pack_clone = Arc::clone(&pack);
        let start = Instant::now();

        let result = tokio::task::spawn_blocking(move || {
            synthesize_with_pack(
                pack_clone.as_ref(),
                &text_owned,
                &opts_owned,
                text_chars,
                text_truncated,
            )
        })
        .await
        .map_err(|e| crate::error::TranscriptionError::internal(format!("TTS synth join: {e}")))?;

        if start.elapsed() > timeout {
            return Err(ProviderError::Other {
                message: format!(
                    "TTS synthesis exceeded timeout ({} ms)",
                    timeout.as_millis()
                ),
            }
            .into());
        }
        result
    }

    async fn preload(&self, model: &str, voice: &str) -> Result<()> {
        lookup_voice(voice)?;
        let _ = self.ensure_loaded(model, self.local_only).await?;
        Ok(())
    }
}

/// Drop helper reserved for future process-global caches.
pub fn clear_tts_session_cache() {}

/// Convenience: synthesize with a one-shot provider.
pub async fn synthesize_local(
    cache_dir: impl Into<PathBuf>,
    text: &str,
    opts: &SynthesisOptions,
) -> Result<SynthesisResult> {
    let provider = LocalTtsProvider::new(cache_dir.into()).with_progress(false);
    provider.synthesize(text, opts).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_text_user_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        let err = p
            .synthesize("  ", &SynthesisOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[tokio::test]
    async fn missing_pack_local_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        let err = p
            .synthesize("Hello", &SynthesisOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err.exit_code(), 2 | 4));
    }
}
