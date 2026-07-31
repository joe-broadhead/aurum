//! TTS voice/model catalogue, integrity pins, and download lock.

use crate::error::{EnvironmentError, ProviderError, Result, UserError};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Default model id (KittenTTS nano int8 ONNX).
pub const DEFAULT_TTS_MODEL: &str = "kitten-nano-int8";
/// Default friendly voice name.
pub const DEFAULT_TTS_VOICE: &str = "Luna";
/// Opt-in curated Kokoro-82M int8 model (JOE-1618). Not the default.
pub const KOKORO_TTS_MODEL: &str = "kokoro-82m-int8";
/// Default friendly voice for the Kokoro catalogue entry.
pub const KOKORO_DEFAULT_VOICE: &str = "Heart";

const HF_BASE: &str = "https://huggingface.co";

/// A downloadable file belonging to a voice pack / model.
#[derive(Debug, Clone, Copy)]
pub struct PackFile {
    pub filename: &'static str,
    pub sha256: &'static str,
    pub approx_bytes: u64,
    /// Absolute download URL. When `None`, resolved via Hugging Face
    /// `{HF_BASE}/{hf_repo}/resolve/main/{filename}`.
    pub url: Option<&'static str>,
}

/// Catalogue entry for a TTS model pack.
#[derive(Debug, Clone, Copy)]
pub struct TtsModelInfo {
    pub id: &'static str,
    pub notes: &'static str,
    /// HuggingFace repo id, e.g. `KittenML/kitten-tts-nano-0.8-int8`.
    pub hf_repo: &'static str,
    pub onnx: PackFile,
    pub voices: PackFile,
    pub config: PackFile,
    pub sample_rate_hz: u32,
    /// Soft upper bound on phoneme tokens (including pads). The loaded voice
    /// matrix may impose a tighter limit; synthesis uses `min(catalogue, pack)`.
    pub max_phoneme_tokens: usize,
    /// Supported BCP-47 language tags for this pack (English-only G2P today).
    pub languages: &'static [&'static str],
    /// Adapter id — prepares multi-adapter catalogues without implementing them.
    pub adapter: &'static str,
    pub license: &'static str,
    /// When false, the model is catalogue-only (multi-adapter prep / tests).
    /// It is not listed for end users and cannot be downloaded.
    pub shipped: bool,
}

/// Friendly voice alias → internal NPZ key.
#[derive(Debug, Clone, Copy)]
pub struct VoiceInfo {
    pub id: &'static str,
    pub internal_key: &'static str,
    pub notes: &'static str,
    /// Model pack this voice belongs to.
    pub model: &'static str,
    pub language: &'static str,
}

/// Placeholder model id for multi-adapter catalogue prep / model-scope tests.
/// Not downloadable and not shown in user-facing lists.
pub const PLACEHOLDER_ADAPTER_MODEL: &str = "adapter-b-placeholder";

/// Pinned KittenTTS nano int8 pack (Apache-2.0 weights).
pub const MODELS: &[TtsModelInfo] = &[
    TtsModelInfo {
        id: DEFAULT_TTS_MODEL,
        notes: "KittenTTS nano int8 ~25MB — default English ONNX",
        hf_repo: "KittenML/kitten-tts-nano-0.8-int8",
        onnx: PackFile {
            filename: "kitten_tts_nano_v0_8.onnx",
            sha256: "f7b0afcbee92870b32b8e0276d855b954dc25470c9f051b376ac7eee537c76fc",
            approx_bytes: 24_369_971,
            url: None,
        },
        voices: PackFile {
            filename: "voices.npz",
            sha256: "8aa7cee235abb0739cb51e6559685f65a4dacd95568833d05699b1633f519b3f",
            approx_bytes: 3_278_902,
            url: None,
        },
        config: PackFile {
            filename: "config.json",
            sha256: "b66006ccbeccd4de5fc3c9272059c47f5725df7215fd889785c03602652fab64",
            approx_bytes: 688,
            url: None,
        },
        sample_rate_hz: 24_000,
        // KittenTTS voice matrices expose ~400 style rows (seq length index).
        max_phoneme_tokens: 400,
        languages: &["en", "en-US"],
        adapter: "kitten-onnx-v1",
        license: "Apache-2.0",
        shipped: true,
    },
    // Curated Kokoro-82M int8 (JOE-1618). Opt-in; Kitten remains default.
    // Pins verified 2026-07-31 against kokoro-onnx model-files-v1.0 + hexgrad config.
    TtsModelInfo {
        id: KOKORO_TTS_MODEL,
        notes: "Kokoro-82M int8 ~88MB ONNX — opt-in higher-quality English (not default)",
        hf_repo: "hexgrad/Kokoro-82M",
        onnx: PackFile {
            filename: "kokoro-v1.0.int8.onnx",
            sha256: "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb",
            approx_bytes: 92_361_271,
            url: Some(concat!(
                "https://github.com/thewh1teagle/kokoro-onnx/releases/download/",
                "model-files-v1.0/kokoro-v1.0.int8.onnx"
            )),
        },
        voices: PackFile {
            filename: "voices-v1.0.bin",
            sha256: "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
            approx_bytes: 28_214_398,
            url: Some(concat!(
                "https://github.com/thewh1teagle/kokoro-onnx/releases/download/",
                "model-files-v1.0/voices-v1.0.bin"
            )),
        },
        config: PackFile {
            filename: "config.json",
            sha256: "5abb01e2403b072bf03d04fde160443e209d7a0dad49a423be15196b9b43c17f",
            approx_bytes: 2_351,
            url: Some(
                "https://huggingface.co/hexgrad/Kokoro-82M/resolve/main/config.json?download=true",
            ),
        },
        sample_rate_hz: 24_000,
        // Kokoro voice matrices are (510, 1, 256) — style rows by sequence length.
        max_phoneme_tokens: 509,
        languages: &["en", "en-US", "en-GB"],
        adapter: "kokoro-onnx-v0",
        license: "Apache-2.0",
        shipped: true,
    },
    // Multi-adapter catalogue slot (no weights). Enables model-scoped voice
    // rejection tests.
    TtsModelInfo {
        id: PLACEHOLDER_ADAPTER_MODEL,
        notes: "Catalogue placeholder for multi-adapter prep (not downloadable)",
        hf_repo: "aurum/not-shipped",
        onnx: PackFile {
            filename: "not-shipped.onnx",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            approx_bytes: 1,
            url: None,
        },
        voices: PackFile {
            filename: "not-shipped.npz",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            approx_bytes: 1,
            url: None,
        },
        config: PackFile {
            filename: "not-shipped.json",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            approx_bytes: 1,
            url: None,
        },
        sample_rate_hz: 24_000,
        max_phoneme_tokens: 400,
        languages: &["en"],
        adapter: "placeholder-v0",
        license: "n/a",
        shipped: false,
    },
];

/// Voices shipped with the default KittenTTS pack.
pub const VOICES: &[VoiceInfo] = &[
    VoiceInfo {
        id: "Bella",
        internal_key: "expr-voice-2-f",
        notes: "female",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Jasper",
        internal_key: "expr-voice-2-m",
        notes: "male",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Luna",
        internal_key: "expr-voice-3-f",
        notes: "female — default",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Bruno",
        internal_key: "expr-voice-3-m",
        notes: "male",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Rosie",
        internal_key: "expr-voice-4-f",
        notes: "female",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Hugo",
        internal_key: "expr-voice-4-m",
        notes: "male",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Kiki",
        internal_key: "expr-voice-5-f",
        notes: "female",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    VoiceInfo {
        id: "Leo",
        internal_key: "expr-voice-5-m",
        notes: "male",
        model: DEFAULT_TTS_MODEL,
        language: "en",
    },
    // Bound only to the placeholder adapter — used for model-scope rejection.
    VoiceInfo {
        id: "PlaceholderVoice",
        internal_key: "placeholder-voice",
        notes: "catalogue-only; not for synthesis",
        model: PLACEHOLDER_ADAPTER_MODEL,
        language: "en",
    },
    // --- Kokoro-82M English catalogue (JOE-1618); internal keys match voices-v1.0.bin ---
    VoiceInfo {
        id: "Heart",
        internal_key: "af_heart",
        notes: "female — Kokoro default",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Alloy",
        internal_key: "af_alloy",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Aoede",
        internal_key: "af_aoede",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "KokoroBella",
        internal_key: "af_bella",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Jessica",
        internal_key: "af_jessica",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Kore",
        internal_key: "af_kore",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Nicole",
        internal_key: "af_nicole",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Nova",
        internal_key: "af_nova",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "River",
        internal_key: "af_river",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Sarah",
        internal_key: "af_sarah",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Sky",
        internal_key: "af_sky",
        notes: "female",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Adam",
        internal_key: "am_adam",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Echo",
        internal_key: "am_echo",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Eric",
        internal_key: "am_eric",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Fenrir",
        internal_key: "am_fenrir",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Liam",
        internal_key: "am_liam",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Michael",
        internal_key: "am_michael",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Onyx",
        internal_key: "am_onyx",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Puck",
        internal_key: "am_puck",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Santa",
        internal_key: "am_santa",
        notes: "male",
        model: KOKORO_TTS_MODEL,
        language: "en-US",
    },
    VoiceInfo {
        id: "Alice",
        internal_key: "bf_alice",
        notes: "female — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Emma",
        internal_key: "bf_emma",
        notes: "female — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Isabella",
        internal_key: "bf_isabella",
        notes: "female — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Lily",
        internal_key: "bf_lily",
        notes: "female — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Daniel",
        internal_key: "bm_daniel",
        notes: "male — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Fable",
        internal_key: "bm_fable",
        notes: "male — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "George",
        internal_key: "bm_george",
        notes: "male — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
    VoiceInfo {
        id: "Lewis",
        internal_key: "bm_lewis",
        notes: "male — British",
        model: KOKORO_TTS_MODEL,
        language: "en-GB",
    },
];

pub fn available_model_names() -> String {
    MODELS
        .iter()
        .filter(|m| m.shipped)
        .map(|m| m.id)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn available_voice_names() -> String {
    VOICES
        .iter()
        .filter(|v| {
            MODELS
                .iter()
                .any(|m| m.shipped && m.id.eq_ignore_ascii_case(v.model))
        })
        .map(|v| v.id)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn lookup_model(name: &str) -> Result<&'static TtsModelInfo> {
    let key = name.trim().to_ascii_lowercase();
    MODELS
        .iter()
        .find(|m| m.id.eq_ignore_ascii_case(&key))
        .ok_or_else(|| {
            UserError::InvalidModel {
                model: name.to_string(),
                available: available_model_names(),
            }
            .into()
        })
}

/// Lookup a **shipped** model suitable for download/synthesis.
pub fn lookup_shipped_model(name: &str) -> Result<&'static TtsModelInfo> {
    let info = lookup_model(name)?;
    if !info.shipped {
        return Err(UserError::Other {
            message: format!(
                "TTS model '{}' is a catalogue placeholder and is not downloadable yet\n  \
                 Hint: available models: {}",
                info.id,
                available_model_names()
            ),
        }
        .into());
    }
    Ok(info)
}

pub fn lookup_voice(name: &str) -> Result<&'static VoiceInfo> {
    let key = name.trim();
    VOICES
        .iter()
        .find(|v| v.id.eq_ignore_ascii_case(key) || v.internal_key.eq_ignore_ascii_case(key))
        .ok_or_else(|| {
            UserError::Other {
                message: format!(
                    "unknown TTS voice '{name}'\n  Hint: available voices: {}",
                    available_voice_names()
                ),
            }
            .into()
        })
}

/// Resolve a voice **within** a selected model pack.
///
/// Case-insensitive aliases resolve without losing canonical model/voice IDs.
/// A voice that belongs to another model is rejected deterministically.
pub fn resolve_voice_for_model(
    model: &str,
    voice: &str,
) -> Result<(&'static TtsModelInfo, &'static VoiceInfo)> {
    let model_info = lookup_model(model)?;
    let voice_info = lookup_voice(voice)?;
    if !voice_info.model.eq_ignore_ascii_case(model_info.id) {
        return Err(UserError::Other {
            message: format!(
                "voice '{}' belongs to model '{}', not '{}'\n  \
                 Hint: available voices for {}: {}",
                voice_info.id,
                voice_info.model,
                model_info.id,
                model_info.id,
                voices_for_model(model_info.id)
            ),
        }
        .into());
    }
    Ok((model_info, voice_info))
}

fn voices_for_model(model_id: &str) -> String {
    VOICES
        .iter()
        .filter(|v| v.model.eq_ignore_ascii_case(model_id))
        .map(|v| v.id)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate speaking rate against the model/engine range before inference.
pub fn validate_speaking_rate(rate: f32) -> Result<f32> {
    use super::validate::{SPEAKING_RATE_MAX, SPEAKING_RATE_MIN};
    if !rate.is_finite() {
        return Err(UserError::Other {
            message: format!(
                "invalid speaking rate (not finite)\n  Hint: use a value in {SPEAKING_RATE_MIN}..={SPEAKING_RATE_MAX}."
            ),
        }
        .into());
    }
    if !(SPEAKING_RATE_MIN..=SPEAKING_RATE_MAX).contains(&rate) {
        return Err(UserError::Other {
            message: format!(
                "speaking rate {rate} is outside supported range {SPEAKING_RATE_MIN}..={SPEAKING_RATE_MAX}"
            ),
        }
        .into());
    }
    Ok(rate)
}

/// `<cache>/tts/`
pub fn tts_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("tts")
}

/// `<cache>/tts/<model-id>/`
pub fn model_pack_dir(cache_dir: &Path, info: &TtsModelInfo) -> PathBuf {
    tts_cache_dir(cache_dir).join(info.id)
}

pub fn onnx_path(cache_dir: &Path, info: &TtsModelInfo) -> PathBuf {
    model_pack_dir(cache_dir, info).join(info.onnx.filename)
}

pub fn voices_path(cache_dir: &Path, info: &TtsModelInfo) -> PathBuf {
    model_pack_dir(cache_dir, info).join(info.voices.filename)
}

pub fn config_path(cache_dir: &Path, info: &TtsModelInfo) -> PathBuf {
    model_pack_dir(cache_dir, info).join(info.config.filename)
}

#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub info: &'static TtsModelInfo,
    pub cached: bool,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct VoiceStatus {
    pub info: &'static VoiceInfo,
    pub cached: bool,
    pub model_cached: bool,
}

/// True when the full pack (onnx + voices + config) verifies against pins.
pub fn is_pack_cached(cache_dir: &Path, model_name: &str) -> bool {
    let Ok(info) = lookup_model(model_name) else {
        return false;
    };
    verify_pack(cache_dir, info).is_ok()
}

fn verify_pack(cache_dir: &Path, info: &TtsModelInfo) -> Result<()> {
    verify_file(&onnx_path(cache_dir, info), info.onnx.sha256, info.id)?;
    verify_file(&voices_path(cache_dir, info), info.voices.sha256, info.id)?;
    verify_file(&config_path(cache_dir, info), info.config.sha256, info.id)?;
    Ok(())
}

fn verify_file(path: &Path, expected_sha: &str, model: &str) -> Result<()> {
    if !path.exists() {
        return Err(UserError::ModelNotCached {
            model: model.to_string(),
        }
        .into());
    }
    if !verify_against_expected(path, expected_sha) {
        return Err(ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!(
                "cached file {} failed pinned sha256 check ({expected_sha})",
                path.display()
            ),
        }
        .into());
    }
    Ok(())
}

fn verify_against_expected(path: &Path, expected: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hex::encode(hasher.finalize()) == expected
}

pub fn list_models(cache_dir: &Path) -> Vec<ModelStatus> {
    MODELS
        .iter()
        .filter(|info| info.shipped)
        .map(|info| {
            let path = model_pack_dir(cache_dir, info);
            let cached = verify_pack(cache_dir, info).is_ok();
            let size_bytes = if cached {
                let a = fs::metadata(onnx_path(cache_dir, info))
                    .map(|m| m.len())
                    .unwrap_or(0);
                let b = fs::metadata(voices_path(cache_dir, info))
                    .map(|m| m.len())
                    .unwrap_or(0);
                Some(a + b)
            } else {
                None
            };
            ModelStatus {
                info,
                cached,
                path,
                size_bytes,
            }
        })
        .collect()
}

pub fn list_voices(cache_dir: &Path) -> Vec<VoiceStatus> {
    VOICES
        .iter()
        .filter(|info| {
            MODELS
                .iter()
                .any(|m| m.shipped && m.id.eq_ignore_ascii_case(info.model))
        })
        .map(|info| {
            let model_cached = is_pack_cached(cache_dir, info.model);
            VoiceStatus {
                info,
                cached: model_cached,
                model_cached,
            }
        })
        .collect()
}

pub fn format_model_list(cache_dir: &Path) -> String {
    let rows = list_models(cache_dir);
    let mut out = String::from("Local TTS models (cache: ");
    out.push_str(&tts_cache_dir(cache_dir).display().to_string());
    out.push_str(")\n\n");
    out.push_str(&format!(
        "{:<22} {:>10}  {:<8}  {:<16}  {:<8}  {}\n",
        "NAME", "SIZE", "STATUS", "ADAPTER", "TRUST", "NOTES"
    ));
    out.push_str(&format!(
        "{:<22} {:>10}  {:<8}  {:<16}  {:<8}  {}\n",
        "----", "----", "------", "-------", "-----", "-----"
    ));
    for row in rows {
        let size = format_bytes(row.info.onnx.approx_bytes + row.info.voices.approx_bytes);
        let status = if row.cached { "cached" } else { "—" };
        out.push_str(&format!(
            "{:<22} {:>10}  {:<8}  {:<16}  {:<8}  {} [{}] provenance=builtin\n",
            row.info.id,
            size,
            status,
            row.info.adapter,
            "builtin",
            row.info.notes,
            row.info.license
        ));
    }
    out.push_str(&format!(
        "\nDefault model: `{DEFAULT_TTS_MODEL}` (trust=builtin, KittenTTS).\n\
         Opt-in higher quality: `{KOKORO_TTS_MODEL}` (Kokoro-82M int8, adapter kokoro-onnx-v0).\n\
         Custom models use [[tts.custom_models]] and never shadow built-ins (see `aurum tts adapters`).\n\
         Bare .onnx paths are not supported — use a pack + manifest + known adapter.\n\
         Licenses: Kitten/Kokoro weights Apache-2.0; G2P via MIT misaki-rs (no GPL espeak).\n"
    ));
    out
}

pub fn format_voice_list(cache_dir: &Path) -> String {
    let rows = list_voices(cache_dir);
    let mut out = String::from("Local TTS voices (model-scoped)\n\n");
    out.push_str(&format!(
        "{:<12} {:<20} {:<8}  {:<8}  {:<8}  {}\n",
        "NAME", "MODEL", "LANG", "STATUS", "ADAPTER", "NOTES"
    ));
    out.push_str(&format!(
        "{:<12} {:<20} {:<8}  {:<8}  {:<8}  {}\n",
        "----", "-----", "----", "------", "-------", "-----"
    ));
    for row in rows {
        let status = if row.cached { "cached" } else { "—" };
        let adapter = lookup_model(row.info.model)
            .map(|m| m.adapter)
            .unwrap_or("?");
        out.push_str(&format!(
            "{:<12} {:<20} {:<8}  {:<8}  {:<8}  {}\n",
            row.info.id, row.info.model, row.info.language, status, adapter, row.info.notes
        ));
    }
    out.push_str(&format!(
        "\nDefault voice: `{DEFAULT_TTS_VOICE}` on model `{DEFAULT_TTS_MODEL}`.\n\
         Voices are validated against the selected model; mismatches are rejected.\n"
    ));
    out
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let n = n as f64;
    if n >= MB {
        format!("{:.0} MB", n / MB)
    } else {
        format!("{:.0} KB", n / KB)
    }
}

/// Ensure the model pack is present (download + pin verify). Returns pack dir.
pub async fn ensure_voice_pack(
    cache_dir: &Path,
    model_name: &str,
    show_progress: bool,
    local_only: bool,
) -> Result<PathBuf> {
    let info = lookup_shipped_model(model_name)?;
    let pack_dir = model_pack_dir(cache_dir, info);

    if verify_pack(cache_dir, info).is_ok() {
        tracing::info!(model = info.id, path = %pack_dir.display(), "using cached TTS pack");
        return Ok(pack_dir);
    }

    if local_only {
        return Err(UserError::ModelNotCached {
            model: model_name.to_string(),
        }
        .into());
    }

    fs::create_dir_all(&pack_dir).map_err(|e| EnvironmentError::DirectoryAccess {
        path: pack_dir.display().to_string(),
        reason: e.to_string(),
    })?;

    let lock_path = pack_dir.join(".download.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| EnvironmentError::DirectoryAccess {
            path: lock_path.display().to_string(),
            reason: e.to_string(),
        })?;

    if show_progress {
        eprintln!("aurum: waiting for TTS download lock ({}) …", info.id);
    }
    lock_file.lock().map_err(|e| EnvironmentError::Other {
        message: format!("failed to acquire TTS download lock: {e}"),
    })?;

    // Re-check after lock.
    if verify_pack(cache_dir, info).is_ok() {
        let _ = lock_file.unlock();
        return Ok(pack_dir);
    }

    if local_only {
        let _ = lock_file.unlock();
        return Err(UserError::ModelNotCached {
            model: model_name.to_string(),
        }
        .into());
    }

    if show_progress {
        eprintln!(
            "aurum: downloading TTS pack `{}` (~{}) — first run only …",
            info.id,
            format_bytes(info.onnx.approx_bytes + info.voices.approx_bytes)
        );
    }

    let files = [info.config, info.onnx, info.voices];
    for file in files {
        let dest = pack_dir.join(file.filename);
        if dest.exists() && verify_against_expected(&dest, file.sha256) {
            continue;
        }
        let url = match file.url {
            Some(absolute) => absolute.to_string(),
            None => format!(
                "{HF_BASE}/{}/resolve/main/{}?download=true",
                info.hf_repo, file.filename
            ),
        };
        if let Err(e) = download_pinned(
            info.id,
            &url,
            &dest,
            file.sha256,
            file.approx_bytes,
            show_progress,
        )
        .await
        {
            let _ = lock_file.unlock();
            return Err(e);
        }
    }

    let _ = lock_file.unlock();
    verify_pack(cache_dir, info)?;
    Ok(pack_dir)
}

/// Hard download size cap from the pinned catalogue `approx_bytes`.
///
/// Content-Length must never raise this: a huge/false header would otherwise
/// allow unbounded disk fill before the SHA check runs.
pub(crate) fn download_byte_cap(approx_bytes: u64) -> u64 {
    const FACTOR: u64 = 3;
    const FLOOR: u64 = 1_000_000;
    approx_bytes.saturating_mul(FACTOR).max(FLOOR)
}

async fn download_pinned(
    model: &str,
    url: &str,
    dest: &Path,
    expected_sha: &str,
    approx_bytes: u64,
    show_progress: bool,
) -> Result<()> {
    tracing::info!(%url, path = %dest.display(), "downloading TTS asset");

    let partial = dest.with_extension(format!(
        "partial.{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    if partial.exists() {
        let _ = fs::remove_file(&partial);
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!("http client error: {e}"),
        })?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!("request failed: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!("HTTP {}", response.status()),
        }
        .into());
    }

    // Progress total is advisory only — never raise the hard size cap from Content-Length.
    let progress_total = response
        .content_length()
        .filter(|&n| n > 0 && n <= download_byte_cap(approx_bytes))
        .unwrap_or(approx_bytes);
    let pb = if show_progress {
        let pb = ProgressBar::new(progress_total);
        pb.set_style(
            ProgressStyle::with_template(
                "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
        );
        pb.set_message(format!(
            "Downloading {}",
            dest.file_name().and_then(|s| s.to_str()).unwrap_or("file")
        ));
        Some(pb)
    } else {
        None
    };

    let mut file = File::create(&partial).map_err(|e| EnvironmentError::DirectoryAccess {
        path: partial.display().to_string(),
        reason: e.to_string(),
    })?;

    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    // Fail-closed size cap from pinned catalogue size only (not Content-Length).
    let max_allowed = download_byte_cap(approx_bytes);

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!("stream error: {e}"),
        })?;
        file.write_all(&chunk)
            .map_err(|e| EnvironmentError::DiskSpace {
                path: partial.display().to_string(),
                reason: e.to_string(),
            })?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        if downloaded > max_allowed {
            let _ = fs::remove_file(&partial);
            return Err(ProviderError::ModelDownload {
                model: model.to_string(),
                reason: format!("download exceeded size cap ({downloaded} > {max_allowed})"),
            }
            .into());
        }
        if let Some(pb) = &pb {
            pb.set_position(downloaded.min(progress_total));
        }
    }
    file.flush().ok();
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if digest != expected_sha {
        let _ = fs::remove_file(&partial);
        return Err(ProviderError::ModelDownload {
            model: model.to_string(),
            reason: format!(
                "sha256 mismatch for {} (got {digest}, expected {expected_sha}) — refusing to cache",
                dest.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file")
            ),
        }
        .into());
    }

    // Windows cannot rename over an existing destination (re-download / pin repair).
    if dest.exists() {
        fs::remove_file(dest).map_err(|e| EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: format!("failed to replace existing pack file: {e}"),
        })?;
    }
    if let Err(e) = fs::rename(&partial, dest) {
        let _ = fs::remove_file(&partial);
        return Err(EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: e.to_string(),
        }
        .into());
    }

    if let Some(pb) = pb {
        pb.finish_with_message(format!(
            "Downloaded {} ({downloaded} bytes)",
            dest.file_name().and_then(|s| s.to_str()).unwrap_or("file")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_default_model() {
        let m = lookup_model(DEFAULT_TTS_MODEL).unwrap();
        assert_eq!(m.sample_rate_hz, 24_000);
        assert!(!m.onnx.sha256.is_empty());
    }

    #[test]
    fn lookup_voices_case_insensitive() {
        assert_eq!(lookup_voice("luna").unwrap().id, "Luna");
        assert_eq!(lookup_voice("expr-voice-3-f").unwrap().id, "Luna");
        assert!(lookup_voice("nope").is_err());
    }

    #[test]
    fn resolve_voice_for_default_model() {
        let (m, v) = resolve_voice_for_model(DEFAULT_TTS_MODEL, "luna").unwrap();
        assert_eq!(m.id, DEFAULT_TTS_MODEL);
        assert_eq!(v.id, "Luna");
        assert_eq!(v.model, DEFAULT_TTS_MODEL);
    }

    #[test]
    fn resolve_voice_unknown_model() {
        assert!(resolve_voice_for_model("no-such-model", "Luna").is_err());
    }

    #[test]
    fn resolve_rejects_voice_bound_to_another_model() {
        let err = resolve_voice_for_model(DEFAULT_TTS_MODEL, "PlaceholderVoice").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("belongs to model"), "{err}");
        // Actionable guidance should list voices for the requested model.
        assert!(err.to_string().contains("Luna") || err.to_string().contains("available"));
    }

    #[test]
    fn resolve_rejects_default_voice_on_placeholder_model() {
        let err = resolve_voice_for_model(PLACEHOLDER_ADAPTER_MODEL, "Luna").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("belongs to model"), "{err}");
    }

    #[test]
    fn lists_hide_placeholder_models_and_voices() {
        let s = format_model_list(Path::new("/tmp/aurum-tts-test-cache"));
        assert!(s.contains(DEFAULT_TTS_MODEL));
        assert!(!s.contains(PLACEHOLDER_ADAPTER_MODEL));
        let v = format_voice_list(Path::new("/tmp/aurum-tts-test-cache"));
        assert!(v.contains("Luna"));
        assert!(!v.contains("PlaceholderVoice"));
    }

    #[test]
    fn unshipped_model_not_downloadable() {
        assert!(lookup_shipped_model(PLACEHOLDER_ADAPTER_MODEL).is_err());
        assert!(lookup_model(PLACEHOLDER_ADAPTER_MODEL).is_ok());
    }

    #[test]
    fn speaking_rate_bounds() {
        assert!(validate_speaking_rate(1.0).is_ok());
        assert!(validate_speaking_rate(0.1).is_err());
        assert!(validate_speaking_rate(9.0).is_err());
        assert!(validate_speaking_rate(f32::NAN).is_err());
    }

    #[test]
    fn pin_mismatch_fails() {
        let dir = tempfile::tempdir().unwrap();
        let info = lookup_model(DEFAULT_TTS_MODEL).unwrap();
        let pack = model_pack_dir(dir.path(), info);
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join(info.onnx.filename), b"not-a-model").unwrap();
        fs::write(pack.join(info.voices.filename), b"nope").unwrap();
        fs::write(pack.join(info.config.filename), b"{}").unwrap();
        assert!(verify_pack(dir.path(), info).is_err());
    }

    #[test]
    fn lists_format() {
        let s = format_model_list(Path::new("/tmp/aurum-tts-test-cache"));
        assert!(s.contains(DEFAULT_TTS_MODEL));
        let v = format_voice_list(Path::new("/tmp/aurum-tts-test-cache"));
        assert!(v.contains("Luna"));
    }

    #[test]
    fn download_cap_uses_pinned_approx_not_content_length() {
        // Small pin → floor of 1 MiB.
        assert_eq!(download_byte_cap(100), 1_000_000);
        // Typical pin → 3× approx.
        assert_eq!(download_byte_cap(10_000_000), 30_000_000);
        // Cap must not grow with a forged Content-Length (helper ignores it).
        let pin = 24_369_971u64;
        let forged_cl = 10_u64.pow(12);
        assert!(download_byte_cap(pin) < forged_cl);
        assert_eq!(download_byte_cap(pin), pin.saturating_mul(3));
    }
}
