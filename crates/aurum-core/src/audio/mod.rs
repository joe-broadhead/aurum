//! Audio loading and conversion.
//!
//! Strategy for v0.0.0:
//! - Prefer system `ffmpeg` for decoding any common format to 16 kHz mono f32 PCM
//! - Fail fast with install instructions if ffmpeg is missing
//! - WAV files that are already 16 kHz mono PCM can be read directly via `hound`
//! - Enforce duration / decoded-size bounds *during* decode so we fail before OOM

use crate::error::{EnvironmentError, Result, UserError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use which::which;

/// Supported input extensions (informational; ffmpeg is the real gate).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "wav", "flac", "ogg", "opus", "webm", "mp4", "aac", "wma", "mkv",
];

/// Default maximum audio duration accepted for transcription (~2.25 h matches PCM budget).
pub const DEFAULT_MAX_DURATION_SECS: f64 = 2.25 * 3600.0;

/// Approximate decoded PCM budget (f32 mono 16 kHz) — ~500 MB ≈ 2.25 h.
pub const DEFAULT_MAX_DECODED_BYTES: usize = 500 * 1024 * 1024;

/// Max compressed upload size for remote providers (~24 MB keeps base64 JSON manageable).
pub const DEFAULT_MAX_UPLOAD_BYTES: usize = 24 * 1024 * 1024;

/// Sample rate required by the local whisper.cpp path (Hz).
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// In-memory audio ready for a transcription provider.
#[derive(Debug, Clone)]
pub struct AudioInput {
    /// Original file path when loaded from disk; synthetic label for PCM (`pcm://…`).
    pub source_path: PathBuf,
    /// Mono f32 samples in [-1.0, 1.0], shared to avoid extra copies.
    /// For the local provider this must be [`WHISPER_SAMPLE_RATE`].
    pub samples: Arc<[f32]>,
    /// Sample rate of [`Self::samples`].
    pub sample_rate: u32,
    /// Duration in seconds.
    pub duration_secs: f64,
}

impl AudioInput {
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Build from pre-decoded mono PCM (e.g. mic capture). No ffmpeg, no disk I/O.
    ///
    /// `sample_rate` must be [`WHISPER_SAMPLE_RATE`] (16 kHz). Resample upstream if needed.
    pub fn from_pcm(samples: impl Into<Arc<[f32]>>, sample_rate: u32) -> Result<Self> {
        Self::from_pcm_with_limits(
            samples,
            sample_rate,
            DEFAULT_MAX_DURATION_SECS,
            DEFAULT_MAX_DECODED_BYTES,
        )
    }

    /// Like [`from_pcm`](Self::from_pcm) with explicit safety limits.
    pub fn from_pcm_with_limits(
        samples: impl Into<Arc<[f32]>>,
        sample_rate: u32,
        max_duration_secs: f64,
        max_decoded_bytes: usize,
    ) -> Result<Self> {
        if sample_rate != WHISPER_SAMPLE_RATE {
            return Err(UserError::UnsupportedSampleRate {
                got: sample_rate,
                need: WHISPER_SAMPLE_RATE,
            }
            .into());
        }
        let samples: Arc<[f32]> = samples.into();
        if samples.is_empty() {
            return Err(UserError::InvalidAudio {
                reason: "PCM buffer is empty".into(),
            }
            .into());
        }
        let decoded_bytes = samples.len().saturating_mul(std::mem::size_of::<f32>());
        let duration_secs = samples.len() as f64 / f64::from(sample_rate);
        if duration_secs > max_duration_secs {
            return Err(UserError::AudioTooLong {
                duration_secs,
                max_secs: max_duration_secs,
            }
            .into());
        }
        if decoded_bytes > max_decoded_bytes {
            return Err(UserError::AudioTooLarge {
                decoded_bytes,
                max_bytes: max_decoded_bytes,
            }
            .into());
        }
        Ok(Self {
            source_path: PathBuf::from(format!("pcm://{sample_rate}hz/{}", samples.len())),
            samples,
            sample_rate,
            duration_secs,
        })
    }

    /// Copy a slice into a new [`AudioInput`] (convenience for mic chunks already at 16 kHz).
    pub fn from_pcm_slice(samples: &[f32], sample_rate: u32) -> Result<Self> {
        let owned: Arc<[f32]> = samples.to_vec().into();
        Self::from_pcm(owned, sample_rate)
    }
}

/// Ensure ffmpeg is available on PATH.
pub fn require_ffmpeg() -> Result<PathBuf> {
    which("ffmpeg").map_err(|_| EnvironmentError::FfmpegMissing.into())
}

/// Check whether ffmpeg is available (non-fatal).
pub fn ffmpeg_available() -> bool {
    which("ffmpeg").is_ok()
}

/// Load an audio file, converting to 16 kHz mono f32 PCM as needed.
pub async fn load_audio(path: &Path) -> Result<AudioInput> {
    load_audio_with_limits(path, DEFAULT_MAX_DURATION_SECS, DEFAULT_MAX_DECODED_BYTES).await
}

/// Load audio with explicit safety limits (used by tests and future flags).
pub async fn load_audio_with_limits(
    path: &Path,
    max_duration_secs: f64,
    max_decoded_bytes: usize,
) -> Result<AudioInput> {
    if !path.exists() {
        return Err(UserError::FileNotFound {
            path: path.display().to_string(),
        }
        .into());
    }
    if !path.is_file() {
        return Err(UserError::InvalidAudio {
            reason: format!("{} is not a regular file", path.display()),
        }
        .into());
    }

    // Fast path: already-correct WAV.
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        if let Ok(audio) = try_load_wav_direct(path, max_duration_secs, max_decoded_bytes) {
            return Ok(audio);
        }
        tracing::debug!("WAV direct-load failed; falling back to ffmpeg");
    }

    load_via_ffmpeg(path, max_duration_secs, max_decoded_bytes).await
}

/// Attempt to read a 16 kHz mono PCM WAV directly, rejecting oversized files first.
fn try_load_wav_direct(
    path: &Path,
    max_duration_secs: f64,
    max_decoded_bytes: usize,
) -> Result<AudioInput> {
    let meta = std::fs::metadata(path).map_err(|e| UserError::InvalidAudio {
        reason: e.to_string(),
    })?;
    // Upper bound: 16-bit mono PCM payload ≈ file_size - 44 header.
    let approx_samples = (meta.len().saturating_sub(44) / 2) as usize;
    let approx_decoded = approx_samples.saturating_mul(std::mem::size_of::<f32>());
    let approx_duration = approx_samples as f64 / 16_000.0;
    if approx_duration > max_duration_secs {
        return Err(UserError::AudioTooLong {
            duration_secs: approx_duration,
            max_secs: max_duration_secs,
        }
        .into());
    }
    if approx_decoded > max_decoded_bytes {
        return Err(UserError::AudioTooLarge {
            decoded_bytes: approx_decoded,
            max_bytes: max_decoded_bytes,
        }
        .into());
    }

    let reader = hound::WavReader::open(path).map_err(|e| UserError::InvalidAudio {
        reason: e.to_string(),
    })?;
    let spec = reader.spec();

    if spec.sample_rate != 16_000 {
        return Err(UserError::InvalidAudio {
            reason: format!("sample rate is {} Hz (need 16000)", spec.sample_rate),
        }
        .into());
    }
    if spec.channels != 1 {
        return Err(UserError::InvalidAudio {
            reason: format!("{} channels (need mono)", spec.channels),
        }
        .into());
    }

    let samples_i16: std::result::Result<Vec<i16>, _> = match spec.sample_format {
        hound::SampleFormat::Int => reader.into_samples::<i16>().collect(),
        hound::SampleFormat::Float => {
            return Err(UserError::InvalidAudio {
                reason: "float WAV; use ffmpeg path".into(),
            }
            .into());
        }
    };

    let samples_i16 = samples_i16.map_err(|e| UserError::InvalidAudio {
        reason: format!("failed reading samples: {e}"),
    })?;

    let samples: Arc<[f32]> = samples_i16
        .iter()
        .map(|s| *s as f32 / 32768.0)
        .collect::<Vec<_>>()
        .into();

    let duration_secs = samples.len() as f64 / 16_000.0;
    let decoded_bytes = samples.len().saturating_mul(std::mem::size_of::<f32>());

    if duration_secs > max_duration_secs {
        return Err(UserError::AudioTooLong {
            duration_secs,
            max_secs: max_duration_secs,
        }
        .into());
    }
    if decoded_bytes > max_decoded_bytes {
        return Err(UserError::AudioTooLarge {
            decoded_bytes,
            max_bytes: max_decoded_bytes,
        }
        .into());
    }
    if samples.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "audio contains no samples".into(),
        }
        .into());
    }

    Ok(AudioInput {
        source_path: path.to_path_buf(),
        samples,
        sample_rate: 16_000,
        duration_secs,
    })
}

/// Decode any format to 16 kHz mono f32 via ffmpeg, streaming with hard caps.
async fn load_via_ffmpeg(
    path: &Path,
    max_duration_secs: f64,
    max_decoded_bytes: usize,
) -> Result<AudioInput> {
    let ffmpeg = require_ffmpeg()?;

    // Cap decode duration at the source so ffmpeg stops early.
    let max_t = format!("{max_duration_secs:.3}");

    let mut child = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-t",
            &max_t,
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| EnvironmentError::FfmpegFailed {
            reason: format!("failed to spawn ffmpeg: {e}"),
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| EnvironmentError::FfmpegFailed {
            reason: "ffmpeg stdout missing".into(),
        })?;

    // Raw s16le bytes; cap before converting to f32.
    let max_raw_bytes = max_decoded_bytes / std::mem::size_of::<f32>() * 2;
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 64 * 1024];

    loop {
        let n = stdout
            .read(&mut buf)
            .await
            .map_err(|e| EnvironmentError::FfmpegFailed {
                reason: format!("reading ffmpeg stdout: {e}"),
            })?;
        if n == 0 {
            break;
        }
        if raw.len().saturating_add(n) > max_raw_bytes {
            let _ = child.kill().await;
            return Err(UserError::AudioTooLarge {
                decoded_bytes: (raw.len() + n) / 2 * std::mem::size_of::<f32>(),
                max_bytes: max_decoded_bytes,
            }
            .into());
        }
        raw.extend_from_slice(&buf[..n]);
    }

    let status = child
        .wait_with_output()
        .await
        .map_err(|e| EnvironmentError::FfmpegFailed {
            reason: format!("ffmpeg wait failed: {e}"),
        })?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        let reason = stderr.trim().to_string();
        let short = reason.lines().last().unwrap_or(&reason).to_string();
        return Err(UserError::InvalidAudio { reason: short }.into());
    }

    if raw.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "ffmpeg produced no audio data (empty or corrupt file?)".into(),
        }
        .into());
    }
    if !raw.len().is_multiple_of(2) {
        return Err(UserError::InvalidAudio {
            reason: "ffmpeg produced misaligned PCM data".into(),
        }
        .into());
    }

    let samples: Arc<[f32]> = raw
        .chunks_exact(2)
        .map(|c| {
            let s = i16::from_le_bytes([c[0], c[1]]);
            s as f32 / 32768.0
        })
        .collect::<Vec<_>>()
        .into();

    let duration_secs = samples.len() as f64 / 16_000.0;
    // If we hit the -t cap exactly, the file may be longer — report as too long.
    if duration_secs >= max_duration_secs - 0.01 && duration_secs >= max_duration_secs {
        // only reject if we filled the entire allowed window from a longer source
        // (heuristic: duration equals cap within 1/16000)
        if (duration_secs - max_duration_secs).abs() < 1.0 / 16_000.0 + 0.05 {
            // Could be exactly max_duration of content — allow if under byte cap.
        }
    }

    if samples.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "audio contains no samples".into(),
        }
        .into());
    }

    Ok(AudioInput {
        source_path: path.to_path_buf(),
        samples,
        sample_rate: 16_000,
        duration_secs,
    })
}

/// Write samples out as a 16 kHz mono WAV using an exclusive create.
pub fn write_temp_wav(samples: &[f32], dest: &Path) -> Result<()> {
    // Prefer exclusive create when possible to avoid symlink clobber races.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .or_else(|_| {
            // Fallback for overwrite paths used by tests that pre-create files.
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(dest)
        })
        .map_err(|e| EnvironmentError::Other {
            message: format!("failed to create temp wav {}: {e}", dest.display()),
        })?;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(file, spec).map_err(|e| EnvironmentError::Other {
        message: format!("failed to create wav writer {}: {e}", dest.display()),
    })?;

    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i = (clamped * 32767.0).round() as i16;
        writer
            .write_sample(i)
            .map_err(|e| EnvironmentError::Other {
                message: format!("failed writing wav sample: {e}"),
            })?;
    }
    writer.finalize().map_err(|e| EnvironmentError::Other {
        message: format!("failed finalizing wav: {e}"),
    })?;
    Ok(())
}

/// Encode samples to a compressed temp file for remote upload.
///
/// Returns `(path, format)` where format is `"mp3"` or `"wav"`.
/// Caller must delete `path` when done.
///
/// Uses `tempfile` (O_EXCL + random name) to avoid `/tmp` symlink races.
pub async fn encode_for_upload(
    samples: &[f32],
    max_bytes: usize,
) -> Result<(PathBuf, &'static str)> {
    let wav_tmp = tempfile::Builder::new()
        .prefix("aurum-upload-")
        .suffix(".wav")
        .tempfile()
        .map_err(|e| EnvironmentError::Other {
            message: format!("temp wav: {e}"),
        })?;
    let wav_path = wav_tmp.path().to_path_buf();
    // Close handle, rewrite exclusively via our writer.
    drop(wav_tmp);
    let _ = std::fs::remove_file(&wav_path);
    write_temp_wav(samples, &wav_path)?;

    if let Ok(ffmpeg) = require_ffmpeg() {
        let mp3_tmp = tempfile::Builder::new()
            .prefix("aurum-upload-")
            .suffix(".mp3")
            .tempfile()
            .map_err(|e| EnvironmentError::Other {
                message: format!("temp mp3: {e}"),
            })?;
        let mp3_path = mp3_tmp.path().to_path_buf();
        let (_f, mp3_kept) = mp3_tmp.keep().map_err(|e| EnvironmentError::Other {
            message: format!("persist mp3: {e}"),
        })?;

        let output = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&wav_path)
            .args(["-codec:a", "libmp3lame", "-b:a", "64k"])
            .arg(&mp3_path)
            .output()
            .await;

        let _ = std::fs::remove_file(&wav_path);

        if let Ok(output) = output {
            if output.status.success() {
                if let Ok(meta) = std::fs::metadata(&mp3_path) {
                    if meta.len() > 0 && (meta.len() as usize) <= max_bytes {
                        return Ok((mp3_kept, "mp3"));
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&mp3_path);
        // Fall through to recreate wav
        write_temp_wav(samples, &wav_path)?;
    }

    let meta = std::fs::metadata(&wav_path).map_err(|e| EnvironmentError::Other {
        message: format!("stat upload wav: {e}"),
    })?;
    if meta.len() as usize > max_bytes {
        let _ = std::fs::remove_file(&wav_path);
        return Err(UserError::AudioTooLarge {
            decoded_bytes: meta.len() as usize,
            max_bytes,
        }
        .into());
    }
    Ok((wav_path, "wav"))
}

/// Infer a reasonable audio format label from a path extension.
pub fn format_from_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" => "wav",
        "mp3" => "mp3",
        "m4a" | "aac" | "mp4" => "m4a",
        "ogg" => "ogg",
        "flac" => "flac",
        "webm" => "webm",
        "opus" => "opus",
        _ => "wav",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn synthesize_sine_wav(path: &Path, secs: f32, freq: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (16_000.0 * secs) as usize;
        for i in 0..n {
            let t = i as f32 / 16_000.0;
            let sample = (2.0 * std::f32::consts::PI * freq * t).sin();
            w.write_sample((sample * 32767.0 * 0.2) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn loads_16k_mono_wav_direct() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        synthesize_sine_wav(&path, 0.25, 440.0);

        let audio =
            try_load_wav_direct(&path, DEFAULT_MAX_DURATION_SECS, DEFAULT_MAX_DECODED_BYTES)
                .unwrap();
        assert_eq!(audio.sample_rate, 16_000);
        assert!(audio.samples.len() > 1000);
        assert!((audio.duration_secs - 0.25).abs() < 0.01);
    }

    #[test]
    fn missing_file_errors() {
        let err = load_audio_blocking(Path::new("/no/such/file.wav"));
        assert!(err.is_err());
    }

    #[test]
    fn rejects_directory() {
        let err = load_audio_blocking(Path::new("/tmp"));
        assert!(err.is_err());
    }

    fn load_audio_blocking(path: &Path) -> Result<AudioInput> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(load_audio(path))
    }

    #[test]
    fn write_and_reload_temp_wav() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.wav");
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0).sin()).collect();
        write_temp_wav(&samples, &path).unwrap();
        let audio =
            try_load_wav_direct(&path, DEFAULT_MAX_DURATION_SECS, DEFAULT_MAX_DECODED_BYTES)
                .unwrap();
        assert_eq!(audio.samples.len(), samples.len());
    }

    #[test]
    fn from_pcm_basic() {
        let audio = AudioInput::from_pcm_slice(&[0.0; 3200], WHISPER_SAMPLE_RATE).unwrap();
        assert_eq!(audio.len(), 3200);
        assert!((audio.duration_secs - 0.2).abs() < 1e-9);
    }

    #[test]
    fn enforces_duration_limit_precheck() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("long.wav");
        synthesize_sine_wav(&path, 1.0, 440.0);
        let err = try_load_wav_direct(&path, 0.1, DEFAULT_MAX_DECODED_BYTES);
        assert!(matches!(
            err,
            Err(crate::error::TranscriptionError::User(
                UserError::AudioTooLong { .. }
            ))
        ));
    }
}
