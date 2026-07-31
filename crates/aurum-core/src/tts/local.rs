//! Local ONNX KittenTTS provider (MIT binary path — no GPL phonemizer).
//!
//! Session loading is singleflight-coalesced (JOE-1597). Concurrent synthesis is
//! bounded by the process [`ResourceGovernor`] (JOE-1596/1600). Caller timeouts
//! never abandon untracked native work: the permit stays held until the ONNX
//! job returns (honest soft-deadline semantics).

use super::adapter::{TrustMode, ADAPTER_FAKE_SINE_V1, ADAPTER_KITTEN_ONNX_V1};
use super::catalogue::{
    ensure_voice_pack, lookup_model, onnx_path, resolve_voice_for_model, validate_speaking_rate,
    voices_path,
};
use super::chunk::{prepare_tts_chunks, TtsChunk, CHUNK_PAUSE_MS};
use super::conformance::synthesize_fake_sine_ms;
use super::npz::load_voices_npz;
use super::pack::load_pack_dir;
use super::pcm_post::{
    duration_ms_from_pcm, trim_trailing_silence, validate_raw_pcm, TailTrimPolicy, PEAK_LIMIT,
};
use super::provider::{BackendKind, SynthesisOptions, SynthesisProvider, SynthesisResult};
use super::validate::{
    normalize_tts_language, prepare_text, resolve_sample_rate, DEFAULT_MAX_CHARS,
};
use super::wav::peak_guard_f32_to_i16;
use crate::error::{ProviderError, Result, UserError};
use crate::runtime::{LoadKey, OpContext, ResourceGovernor, Singleflight};
use async_trait::async_trait;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// On-device KittenTTS via ONNX Runtime + misaki-rs G2P (no espeak / GPL).
///
/// Pool size is intentionally 1 session per model (serial inference under a
/// mutex). Throughput concurrency is governed by ResourceGovernor TTS permits
/// rather than unbounded parallel ONNX sessions (memory cost of multi-session
/// pools is not free for mobile/desktop defaults).
pub struct LocalTtsProvider {
    cache_dir: PathBuf,
    show_progress: bool,
    local_only: bool,
    max_chars: usize,
    /// Singleflight-backed session cache keyed by model id.
    sessions: Arc<Singleflight<LoadedPack>>,
}

struct LoadedPack {
    session: Mutex<Session>,
    voices: HashMap<String, super::npz::VoiceMatrix>,
    sample_rate_hz: u32,
    /// Catalogue / pack phoneme capacity (min of matrix rows and catalogue).
    max_phoneme_tokens: usize,
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
            sessions: Arc::new(Singleflight::default()),
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

    /// Drop loaded ONNX sessions for this provider (frees ORT graphs held in RAM).
    ///
    /// Safe to call anytime; the next synthesize/preload reloads from the on-disk pack.
    /// Does not delete cached files under the TTS cache directory.
    pub fn clear_sessions(&self) {
        self.sessions.clear();
    }

    async fn ensure_loaded(&self, model: &str, local_only: bool) -> Result<Arc<LoadedPack>> {
        let key = LoadKey::tts(model, self.cache_dir.join(model).display().to_string());
        if let Some(ready) = self.sessions.get_ready(&key) {
            return Ok(ready);
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
        let catalogue_max = info.max_phoneme_tokens;
        let key_for_load = key.clone();
        let model_id = model.to_string();
        let sessions = Arc::clone(&self.sessions);

        // Load on a blocking thread; singleflight ensures one native load per key.
        let loaded = tokio::task::spawn_blocking(move || {
            let gov = ResourceGovernor::process_global();
            sessions.get_or_load(key_for_load, || {
                let _permit = gov.acquire(crate::runtime::PermitKind::ModelLoad, None)?;
                load_pack(
                    &onnx,
                    &voices_file,
                    sample_rate,
                    speed_priors,
                    catalogue_max,
                    &model_id,
                )
            })
        })
        .await
        .map_err(|e| crate::error::TranscriptionError::internal(format!("TTS load join: {e}")))??;

        Ok(loaded)
    }

    /// Load a verified (or explicitly unverified) local model pack for a supported
    /// adapter (JOE-1619). Cache key is isolated from built-in catalogue identities.
    async fn ensure_loaded_from_pack(
        &self,
        pack_dir: &Path,
        allow_unverified: bool,
    ) -> Result<(Arc<LoadedPack>, super::adapter::ModelPackManifest)> {
        let (root, manifest) = load_pack_dir(pack_dir, allow_unverified)?;
        if manifest.adapter_id != ADAPTER_KITTEN_ONNX_V1 {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "local pack override synthesis currently supports adapter \
                     '{ADAPTER_KITTEN_ONNX_V1}' (got '{}'); use `aurum tts inspect` \
                     / conformance for other adapters",
                    manifest.adapter_id
                ),
            }
            .into());
        }
        let onnx_name = manifest
            .artifact("onnx")
            .map(|a| a.filename.as_str())
            .ok_or_else(|| UserError::InvalidConfig {
                reason: "pack missing onnx artifact".into(),
            })?;
        let voices_name = manifest
            .artifact("voices")
            .map(|a| a.filename.as_str())
            .ok_or_else(|| UserError::InvalidConfig {
                reason: "pack missing voices artifact".into(),
            })?;
        let onnx = root.join(onnx_name);
        let voices_file = root.join(voices_name);
        let sample_rate = manifest.sample_rate_hz;
        let catalogue_max = manifest.max_phoneme_tokens;
        let model_id = manifest.model_id.clone();
        // Isolate local packs from built-in cache keys.
        let key = LoadKey::tts(
            format!("local-pack:{}", manifest.model_id),
            root.display().to_string(),
        );
        if let Some(ready) = self.sessions.get_ready(&key) {
            return Ok((ready, manifest));
        }
        let key_for_load = key.clone();
        let sessions = Arc::clone(&self.sessions);
        let speed_priors = load_speed_priors_from_path(
            &root.join(
                manifest
                    .artifact("config")
                    .map(|a| a.filename.as_str())
                    .unwrap_or("config.json"),
            ),
        );

        let loaded = tokio::task::spawn_blocking(move || {
            let gov = ResourceGovernor::process_global();
            sessions.get_or_load(key_for_load, || {
                let _permit = gov.acquire(crate::runtime::PermitKind::ModelLoad, None)?;
                load_pack(
                    &onnx,
                    &voices_file,
                    sample_rate,
                    speed_priors,
                    catalogue_max,
                    &model_id,
                )
            })
        })
        .await
        .map_err(|e| {
            crate::error::TranscriptionError::internal(format!("TTS pack load join: {e}"))
        })??;

        Ok((loaded, manifest))
    }
}

#[allow(clippy::too_many_arguments)]
fn synthesize_with_pack(
    pack: &LoadedPack,
    text: &str,
    opts: &SynthesisOptions,
    voice_internal: &str,
    voice_canonical: &str,
    model_canonical: &str,
    text_chars: usize,
    op: &OpContext,
    adapter: &str,
    trust: TrustMode,
    provenance: &str,
) -> Result<SynthesisResult> {
    op.check()?;

    let rate = validate_speaking_rate(opts.speaking_rate)?;
    let sample_rate = resolve_sample_rate(opts.sample_rate_hz, pack.sample_rate_hz)?;

    let voice_mat = pack
        .voices
        .get(voice_internal)
        .ok_or_else(|| UserError::Other {
            message: format!(
                "voice embedding '{voice_internal}' missing from pack; available: {:?}",
                pack.voices.keys().collect::<Vec<_>>()
            ),
        })?;

    // Voice matrix has one style embedding per supported sequence length.
    // Reserve one row because the token vector includes start/end pads.
    let pack_max = voice_mat.nrows.saturating_sub(1);
    let max_tokens = pack_max.min(pack.max_phoneme_tokens);
    if max_tokens <= 2 {
        return Err(ProviderError::ModelLoad {
            model: model_canonical.to_string(),
            reason: "voice embedding has no usable sequence rows".into(),
        }
        .into());
    }

    let chunks = prepare_tts_chunks(text, max_tokens)?;
    let chunk_count = chunks.len();
    let effective_speed = rate
        * pack
            .speed_priors
            .get(voice_internal)
            .copied()
            .unwrap_or(1.0);
    let pause_samples = (sample_rate as u64)
        .saturating_mul(CHUNK_PAUSE_MS)
        .checked_div(1_000)
        .unwrap_or(0) as usize;

    let mut pcm_f32: Vec<f32> = Vec::new();
    let trim_policy = TailTrimPolicy::default();

    for (index, chunk) in chunks.iter().enumerate() {
        op.check()?;
        let chunk_audio =
            synthesize_chunk_f32(pack, voice_mat, chunk, effective_speed).map_err(|err| {
                ProviderError::Other {
                    message: format!(
                        "TTS chunk {}/{} failed near {:?}: {err}",
                        index + 1,
                        chunks.len(),
                        chunk.text.chars().take(80).collect::<String>()
                    ),
                }
            })?;
        let trimmed = trim_trailing_silence(&chunk_audio, trim_policy);
        if index > 0 {
            pcm_f32.resize(pcm_f32.len().saturating_add(pause_samples), 0.0);
        }
        pcm_f32.extend_from_slice(trimmed);
    }

    validate_raw_pcm(&pcm_f32, sample_rate)?;
    let pcm = peak_guard_f32_to_i16(&pcm_f32, PEAK_LIMIT);
    if pcm.is_empty() {
        return Err(ProviderError::Other {
            message: "synthesis produced empty audio after validation".into(),
        }
        .into());
    }

    let duration_ms = duration_ms_from_pcm(pcm.len(), sample_rate);
    let language = normalize_tts_language(&opts.language).unwrap_or_else(|_| "en".into());

    Ok(SynthesisResult {
        pcm_i16_mono: pcm,
        sample_rate_hz: sample_rate,
        channels: 1,
        backend_kind: BackendKind::Local,
        provider: "local".into(),
        model: model_canonical.to_string(),
        voice: voice_canonical.to_string(),
        language,
        duration_ms,
        text_chars,
        text_truncated: false,
        chunk_count,
        synthesized_chars: text_chars,
        adapter: Some(adapter.into()),
        trust: Some(trust.as_str().into()),
        provenance: Some(provenance.into()),
    })
}

fn synthesize_chunk_f32(
    pack: &LoadedPack,
    voice_mat: &super::npz::VoiceMatrix,
    chunk: &TtsChunk,
    effective_speed: f32,
) -> Result<Vec<f32>> {
    let seq_len = chunk.ids.len();
    // KittenTTS indexes the style embedding by phoneme sequence length.
    let style = voice_mat.style_row(seq_len).to_vec();
    let style_dim = style.len();
    let t_ids = Tensor::<i64>::from_array(([1usize, seq_len], chunk.ids.clone())).map_err(|e| {
        ProviderError::Other {
            message: format!("input_ids tensor: {e}"),
        }
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
    Ok(audio_data.to_vec())
}

fn load_pack(
    onnx: &Path,
    voices_file: &Path,
    sample_rate_hz: u32,
    speed_priors: HashMap<String, f32>,
    catalogue_max_tokens: usize,
    model_label: &str,
) -> Result<LoadedPack> {
    let session = Session::builder()
        .map_err(|e| ProviderError::ModelLoad {
            model: model_label.to_string(),
            reason: format!("ORT session builder: {e}"),
        })?
        .commit_from_file(onnx)
        .map_err(|e| ProviderError::ModelLoad {
            model: model_label.to_string(),
            reason: format!("load ONNX: {e}"),
        })?;
    let voices = load_voices_npz(voices_file)?;
    // Derive capacity from the first voice matrix (all Kitten voices share shape).
    let matrix_max = voices
        .values()
        .map(|m| m.nrows.saturating_sub(1))
        .min()
        .unwrap_or(0);
    let max_phoneme_tokens = if matrix_max == 0 {
        catalogue_max_tokens
    } else {
        matrix_max.min(catalogue_max_tokens)
    };
    Ok(LoadedPack {
        session: Mutex::new(session),
        voices,
        sample_rate_hz,
        max_phoneme_tokens,
        speed_priors,
    })
}

fn load_speed_priors(
    cache_dir: &Path,
    info: &super::catalogue::TtsModelInfo,
) -> HashMap<String, f32> {
    load_speed_priors_from_path(&super::catalogue::config_path(cache_dir, info))
}

fn load_speed_priors_from_path(path: &Path) -> HashMap<String, f32> {
    let Ok(bytes) = std::fs::read(path) else {
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

/// Synthesize via the fake-sine adapter (conformance / custom packs; no ONNX).
fn synthesize_fake_adapter(
    opts: &SynthesisOptions,
    model_id: &str,
    trust: TrustMode,
    provenance: &str,
    text_chars: usize,
) -> Result<SynthesisResult> {
    // Map text length to a short bounded duration (deterministic, no network).
    let duration_ms = ((text_chars as u64).saturating_mul(40)).clamp(50, 2_000);
    let pcm_f32 = synthesize_fake_sine_ms(duration_ms).map_err(|e| ProviderError::Other {
        message: format!("fake-sine synth: {e}"),
    })?;
    validate_raw_pcm(&pcm_f32, 24_000)?;
    let pcm = peak_guard_f32_to_i16(&pcm_f32, PEAK_LIMIT);
    let sample_rate = resolve_sample_rate(opts.sample_rate_hz, 24_000)?;
    let language = normalize_tts_language(&opts.language).unwrap_or_else(|_| "en".into());
    let voice = if opts.voice.trim().is_empty() {
        "Tone".into()
    } else {
        opts.voice.clone()
    };
    let out_duration_ms = duration_ms_from_pcm(pcm.len(), sample_rate);
    Ok(SynthesisResult {
        pcm_i16_mono: pcm,
        sample_rate_hz: sample_rate,
        channels: 1,
        backend_kind: BackendKind::Local,
        provider: "local".into(),
        model: model_id.into(),
        voice,
        language,
        duration_ms: out_duration_ms,
        text_chars,
        text_truncated: false,
        chunk_count: 1,
        synthesized_chars: text_chars,
        adapter: Some(ADAPTER_FAKE_SINE_V1.into()),
        trust: Some(trust.as_str().into()),
        provenance: Some(provenance.into()),
    })
}

/// Resolve a voice for a local pack: prefer catalogue Kitten voices, else pack manifest.
fn resolve_pack_voice(
    manifest: &super::adapter::ModelPackManifest,
    requested: &str,
) -> Result<(String, String)> {
    let req = requested.trim();
    let req = if req.is_empty() {
        super::catalogue::DEFAULT_TTS_VOICE
    } else {
        req
    };
    // Catalogue-friendly voices (Luna, Bella, …) when pack is Kitten-shaped.
    if let Ok((_, v)) = resolve_voice_for_model(super::catalogue::DEFAULT_TTS_MODEL, req) {
        return Ok((v.id.to_string(), v.internal_key.to_string()));
    }
    if let Some(v) = manifest
        .voices
        .iter()
        .find(|v| v.id.eq_ignore_ascii_case(req) || v.internal_key.eq_ignore_ascii_case(req))
    {
        return Ok((v.id.clone(), v.internal_key.clone()));
    }
    let available: Vec<_> = manifest.voices.iter().map(|v| v.id.as_str()).collect();
    Err(UserError::Other {
        message: format!(
            "voice '{req}' not found in pack '{}'; available: {available:?}",
            manifest.model_id
        ),
    }
    .into())
}

#[async_trait]
impl SynthesisProvider for LocalTtsProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    async fn synthesize(&self, text: &str, opts: &SynthesisOptions) -> Result<SynthesisResult> {
        let prepared = prepare_text(text, self.max_chars)?;
        let mut opts = opts.clone();
        opts.language = normalize_tts_language(&opts.language)?;

        // Local pack override path (JOE-1619): never hits network, never shadows
        // built-in cache identity. Bare ONNX is rejected by load_pack_dir.
        if let Some(pack_dir) = opts.pack_dir.clone() {
            let (root, manifest) = load_pack_dir(&pack_dir, opts.allow_unverified)?;
            let _ = root;
            if manifest.adapter_id == ADAPTER_FAKE_SINE_V1 {
                let trust = manifest.trust;
                // Prefer pack identity over the built-in default when the caller
                // did not name a distinct custom model id.
                let model_id = if opts.model.trim().is_empty()
                    || opts.model == super::catalogue::DEFAULT_TTS_MODEL
                {
                    manifest.model_id.clone()
                } else {
                    opts.model.clone()
                };
                if opts.voice.trim().is_empty() || opts.voice == super::catalogue::DEFAULT_TTS_VOICE
                {
                    if let Some(v) = manifest.voices.first() {
                        opts.voice = v.id.clone();
                    } else {
                        opts.voice = "Tone".into();
                    }
                }
                return synthesize_fake_adapter(
                    &opts,
                    &model_id,
                    trust,
                    "local_pack",
                    prepared.text_chars,
                );
            }
            if manifest.adapter_id != ADAPTER_KITTEN_ONNX_V1 {
                return Err(UserError::UnsupportedCapability {
                    provider: "tts".into(),
                    model: manifest.model_id,
                    reason: format!(
                        "adapter '{}' is not enabled for local pack synthesis",
                        manifest.adapter_id
                    ),
                    hint: "use kitten-onnx-v1 or fake-sine-v1 packs; see `aurum tts adapters`"
                        .into(),
                }
                .into());
            }
            // Prefer pack-declared model id for honesty when caller left default.
            let model_canonical = if opts.model == super::catalogue::DEFAULT_TTS_MODEL
                || opts.model.trim().is_empty()
            {
                manifest.model_id.clone()
            } else {
                opts.model.clone()
            };
            // Resolve voice against the built-in Kitten voice table when possible;
            // otherwise require a manifest voice id and use internal_key from pack.
            let (voice_canonical, voice_internal) = resolve_pack_voice(&manifest, &opts.voice)?;
            validate_speaking_rate(opts.speaking_rate)?;
            resolve_sample_rate(opts.sample_rate_hz, manifest.sample_rate_hz)?;
            let trust = manifest.trust;
            let pack = self
                .ensure_loaded_from_pack(&pack_dir, opts.allow_unverified)
                .await?
                .0;

            let timeout = Duration::from_millis(if opts.timeout_ms == 0 {
                super::validate::DEFAULT_TIMEOUT_MS
            } else {
                opts.timeout_ms
            });
            let op = OpContext::from_optional_cancel(opts.cancel.clone())
                .with_deadline_from_now(timeout);
            op.check()?;
            let text_owned = prepared.text.clone();
            let text_chars = prepared.text_chars;
            let opts_owned = opts.clone();
            let pack_clone = Arc::clone(&pack);
            let op_for_worker = op.clone();
            let adapter = ADAPTER_KITTEN_ONNX_V1.to_string();
            let join = tokio::task::spawn_blocking(move || {
                let gov = ResourceGovernor::process_global();
                let _permit = gov.acquire_tts(0, Some(&op_for_worker))?;
                op_for_worker.check()?;
                synthesize_with_pack(
                    pack_clone.as_ref(),
                    &text_owned,
                    &opts_owned,
                    &voice_internal,
                    &voice_canonical,
                    &model_canonical,
                    text_chars,
                    &op_for_worker,
                    &adapter,
                    trust,
                    "local_pack",
                )
            })
            .await
            .map_err(|e| {
                crate::error::TranscriptionError::internal(format!("TTS pack synth join: {e}"))
            })??;
            return Ok(join);
        }

        let (model_info, voice_info) = resolve_voice_for_model(&opts.model, &opts.voice)?;
        // Canonical IDs for honesty JSON / metadata.
        opts.model = model_info.id.to_string();
        opts.voice = voice_info.id.to_string();
        validate_speaking_rate(opts.speaking_rate)?;
        resolve_sample_rate(opts.sample_rate_hz, model_info.sample_rate_hz)?;

        let local_only = opts.local_only || self.local_only;
        let pack = self.ensure_loaded(&opts.model, local_only).await?;

        let timeout = Duration::from_millis(if opts.timeout_ms == 0 {
            super::validate::DEFAULT_TIMEOUT_MS
        } else {
            opts.timeout_ms
        });
        let op =
            OpContext::from_optional_cancel(opts.cancel.clone()).with_deadline_from_now(timeout);
        op.check()?;

        let text_owned = prepared.text.clone();
        let text_chars = prepared.text_chars;
        let opts_owned = opts.clone();
        let pack_clone = Arc::clone(&pack);
        let voice_internal = voice_info.internal_key.to_string();
        let voice_canonical = voice_info.id.to_string();
        let model_canonical = model_info.id.to_string();
        let op_for_worker = op.clone();
        let adapter = model_info.adapter.to_string();

        // Hold the TTS + blocking permit for the entire native job lifetime —
        // even if the caller times out. That keeps concurrency bounded and
        // prevents unlimited background native work accumulation (JOE-1600).
        let join = tokio::task::spawn_blocking(move || {
            let gov = ResourceGovernor::process_global();
            let _permit = gov.acquire_tts(0, Some(&op_for_worker))?;
            op_for_worker.check()?;
            synthesize_with_pack(
                pack_clone.as_ref(),
                &text_owned,
                &opts_owned,
                &voice_internal,
                &voice_canonical,
                &model_canonical,
                text_chars,
                &op_for_worker,
                &adapter,
                TrustMode::Builtin,
                "builtin",
            )
        });

        // Pin the join handle so a soft deadline can detach a reaper without
        // abandoning the permit while native work still runs (JOE-1600).
        let join = join;
        tokio::select! {
            join_res = join => {
                match join_res {
                    Ok(result) => result,
                    Err(e) => Err(crate::error::TranscriptionError::internal(format!(
                        "TTS synth join: {e}"
                    ))),
                }
            }
            _ = tokio::time::sleep(timeout) => {
                // Soft deadline: cooperative cancel so chunk loops exit when
                // possible. The blocking task retains the ResourceGovernor permit
                // until it returns; we cannot safely abort in-process ONNX.
                op.cancel.cancel();
                // Note: `join` was moved into select; when this arm wins, the
                // JoinHandle is dropped. Tokio does not cancel spawn_blocking
                // work on JoinHandle drop — the worker continues and drops the
                // permit when the closure returns. That keeps concurrency honest
                // (permits stay occupied) without a reaper task.
                Err(ProviderError::DeadlineExceeded.into())
            }
        }
    }

    async fn preload(&self, model: &str, voice: &str) -> Result<()> {
        // Same contract as synthesize: model-scoped voice validation first.
        let (model_info, _) = resolve_voice_for_model(model, voice)?;
        let _ = self.ensure_loaded(model_info.id, self.local_only).await?;
        Ok(())
    }
}

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

    #[tokio::test]
    async fn unsupported_language_user_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        let opts = SynthesisOptions {
            language: "fr".into(),
            ..Default::default()
        };
        let err = p.synthesize("Hello", &opts).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("unsupported TTS language"));
    }

    #[tokio::test]
    async fn non_native_sample_rate_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        let opts = SynthesisOptions {
            sample_rate_hz: Some(16_000),
            ..Default::default()
        };
        let err = p.synthesize("Hello", &opts).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("sample rate"));
    }

    #[tokio::test]
    async fn invalid_speaking_rate_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        let opts = SynthesisOptions {
            speaking_rate: 9.0,
            ..Default::default()
        };
        let err = p.synthesize("Hello", &opts).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[tokio::test]
    async fn oversized_text_rejected_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf())
            .with_local_only(true)
            .with_max_chars(10);
        let err = p
            .synthesize(
                "this text is definitely longer than ten",
                &SynthesisOptions::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("too long"));
    }

    #[test]
    fn clear_sessions_is_safe_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalTtsProvider::new(dir.path().to_path_buf()).with_local_only(true);
        p.clear_sessions();
        p.clear_sessions();
    }
}
