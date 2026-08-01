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
///
/// Fields are **private** (JOE-1809). Construct with [`AudioInput::from_pcm`] /
/// [`load_audio`], or [`AudioInput::from_parts_unchecked`] for trusted internal
/// decode paths. Prefer accessors over free mutation.
#[derive(Debug, Clone)]
pub struct AudioInput {
    /// Original file path when loaded from disk; synthetic label for PCM (`pcm://…`).
    source_path: PathBuf,
    /// Mono f32 samples in [-1.0, 1.0], shared to avoid extra copies.
    /// For the local provider this must be [`WHISPER_SAMPLE_RATE`].
    samples: Arc<[f32]>,
    /// Sample rate of [`Self::samples`].
    sample_rate: u32,
    /// Duration in seconds.
    duration_secs: f64,
}

impl AudioInput {
    /// Trusted construction after decode (no re-validation of every sample).
    ///
    /// Prefer [`from_pcm`](Self::from_pcm) for host-facing PCM. Callers must
    /// ensure finite samples, positive sample rate, and duration consistency.
    pub fn from_parts_unchecked(
        source_path: PathBuf,
        samples: Arc<[f32]>,
        sample_rate: u32,
        duration_secs: f64,
    ) -> Self {
        Self {
            source_path,
            samples,
            sample_rate,
            duration_secs,
        }
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn samples(&self) -> &Arc<[f32]> {
        &self.samples
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration_secs
    }

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
        if sample_rate == 0 || sample_rate != WHISPER_SAMPLE_RATE {
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
        // Reject non-finite samples early (JOE-1786 progressive domain hardening).
        for (i, s) in samples.iter().enumerate() {
            if !s.is_finite() {
                return Err(UserError::InvalidAudio {
                    reason: format!("PCM sample[{i}] is not finite"),
                }
                .into());
            }
        }
        let decoded_bytes = samples.len().saturating_mul(std::mem::size_of::<f32>());
        let duration_secs = samples.len() as f64 / f64::from(sample_rate);
        if !duration_secs.is_finite() || duration_secs < 0.0 {
            return Err(UserError::InvalidAudio {
                reason: format!(
                    "computed duration is not a valid non-negative finite value ({duration_secs})"
                ),
            }
            .into());
        }
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

    // Stream i16 → f32 directly into one destination buffer (JOE-1602).
    // Never materialize a complete i16 vector alongside the f32 output.
    let samples: Arc<[f32]> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let mut out: Vec<f32> = Vec::with_capacity(approx_samples.min(max_decoded_bytes / 4));
            for sample in reader.into_samples::<i16>() {
                let s = sample.map_err(|e| UserError::InvalidAudio {
                    reason: format!("failed reading samples: {e}"),
                })?;
                let decoded = out
                    .len()
                    .saturating_add(1)
                    .saturating_mul(std::mem::size_of::<f32>());
                if decoded > max_decoded_bytes {
                    return Err(UserError::AudioTooLarge {
                        decoded_bytes: decoded,
                        max_bytes: max_decoded_bytes,
                    }
                    .into());
                }
                out.push(s as f32 / 32768.0);
            }
            out.into()
        }
        hound::SampleFormat::Float => {
            return Err(UserError::InvalidAudio {
                reason: "float WAV; use ffmpeg path".into(),
            }
            .into());
        }
    };

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

    Ok(AudioInput::from_parts_unchecked(
        path.to_path_buf(),
        samples,
        16_000,
        duration_secs,
    ))
}

/// Default wall-clock deadline for a single FFmpeg decode (JOE-1585).
pub const DEFAULT_FFMPEG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
/// Bounded stderr diagnostic tail retained for user-facing errors.
const STDERR_TAIL_CAP: usize = 8 * 1024;

/// Structured FFmpeg termination reason (JOE-1585).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfmpegTermination {
    Success,
    InvalidMedia,
    LimitExceeded,
    Timeout,
    Cancelled,
    SpawnFailure,
    NonZeroExit,
}

/// Decode any format to 16 kHz mono f32 via supervised FFmpeg (JOE-1585).
///
/// - Shell-free argv, `-nostdin`, protocol restriction for local files
/// - Concurrent stdout/stderr drain with hard caps
/// - Wall-clock deadline; kill+reap on any failure path
async fn load_via_ffmpeg(
    path: &Path,
    max_duration_secs: f64,
    max_decoded_bytes: usize,
) -> Result<AudioInput> {
    load_via_ffmpeg_with_timeout(
        path,
        max_duration_secs,
        max_decoded_bytes,
        DEFAULT_FFMPEG_TIMEOUT,
        None,
    )
    .await
}

/// Supervised FFmpeg decode with explicit deadline and optional cancel flag.
pub async fn load_via_ffmpeg_with_timeout(
    path: &Path,
    max_duration_secs: f64,
    max_decoded_bytes: usize,
    timeout: std::time::Duration,
    cancel: Option<crate::cancel::CancelFlag>,
) -> Result<AudioInput> {
    let ffmpeg = require_ffmpeg()?;
    let max_t = format!("{max_duration_secs:.3}");
    // Restrict demuxer protocols to local files when supported by the build.
    let protocol_whitelist = "file,crypto,data";

    let mut child = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            protocol_whitelist,
            "-i",
        ])
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
        .stdin(Stdio::null())
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
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| EnvironmentError::FfmpegFailed {
            reason: "ffmpeg stderr missing".into(),
        })?;

    let max_raw_bytes = max_decoded_bytes / std::mem::size_of::<f32>() * 2;

    if let Some(flag) = &cancel {
        if flag.is_cancelled() {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(crate::error::ProviderError::Cancelled.into());
        }
    }

    // Concurrent pipe drains. Cancellation is raced *outside* the read futures
    // so a stalled read does not delay cancel until the wall-clock timeout
    // (JOE-1648 fourth-pass residual).
    let stdout_task = async {
        // Stream s16le → f32 on the fly (JOE-1602).
        let max_samples = max_decoded_bytes / std::mem::size_of::<f32>();
        let mut samples: Vec<f32> = Vec::with_capacity(max_samples.min(64 * 1024));
        let mut buf = [0u8; 64 * 1024];
        let mut carry: Option<u8> = None;
        let mut raw_bytes_seen: usize = 0;
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
            raw_bytes_seen = raw_bytes_seen.saturating_add(n);
            if raw_bytes_seen > max_raw_bytes {
                return Err(UserError::AudioTooLarge {
                    decoded_bytes: (raw_bytes_seen / 2) * std::mem::size_of::<f32>(),
                    max_bytes: max_decoded_bytes,
                }
                .into());
            }

            let mut offset = 0usize;
            if let Some(lo) = carry.take() {
                let hi = buf[0];
                offset = 1;
                let s = i16::from_le_bytes([lo, hi]);
                samples.push(s as f32 / 32768.0);
            }

            let rem = &buf[offset..n];
            let pairs = rem.len() / 2;
            if samples.len().saturating_add(pairs) > max_samples {
                return Err(UserError::AudioTooLarge {
                    decoded_bytes: (samples.len() + pairs) * std::mem::size_of::<f32>(),
                    max_bytes: max_decoded_bytes,
                }
                .into());
            }
            for chunk in rem.chunks_exact(2) {
                let s = i16::from_le_bytes([chunk[0], chunk[1]]);
                samples.push(s as f32 / 32768.0);
            }
            if rem.len() % 2 == 1 {
                carry = Some(rem[rem.len() - 1]);
            }
        }
        if carry.is_some() {
            return Err(UserError::InvalidAudio {
                reason: "ffmpeg produced misaligned PCM data".into(),
            }
            .into());
        }
        Ok::<Vec<f32>, crate::error::TranscriptionError>(samples)
    };

    let stderr_task = async {
        let mut tail: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4 * 1024];
        loop {
            let n = stderr
                .read(&mut buf)
                .await
                .map_err(|e| EnvironmentError::FfmpegFailed {
                    reason: format!("reading ffmpeg stderr: {e}"),
                })?;
            if n == 0 {
                break;
            }
            if tail.len() + n > STDERR_TAIL_CAP {
                let drop_n = (tail.len() + n).saturating_sub(STDERR_TAIL_CAP);
                if drop_n < tail.len() {
                    tail.drain(..drop_n);
                } else {
                    tail.clear();
                }
            }
            tail.extend_from_slice(&buf[..n]);
        }
        Ok::<Vec<u8>, crate::error::TranscriptionError>(tail)
    };

    let drains = async { tokio::try_join!(stdout_task, stderr_task) };
    let cancel_flag = cancel.clone();
    let cancel_watch = async move {
        match cancel_flag {
            Some(flag) => loop {
                if flag.is_cancelled() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            },
            None => std::future::pending::<()>().await,
        }
    };

    let drain_outcome: Result<(Vec<f32>, Vec<u8>)> = tokio::select! {
        biased;
        _ = cancel_watch => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(crate::error::ProviderError::Cancelled.into())
        }
        timed = tokio::time::timeout(timeout, drains) => {
            match timed {
                Ok(Ok(pair)) => Ok(pair),
                Ok(Err(e)) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(e)
                }
                Err(_elapsed) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(crate::error::ProviderError::DeadlineExceeded.into())
                }
            }
        }
    };

    let (samples_vec, stderr_bytes) = drain_outcome?;

    // Bound the final wait so a stuck child after pipe close cannot hang forever.
    let status = match tokio::time::timeout(std::time::Duration::from_secs(30), child.wait()).await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return Err(EnvironmentError::FfmpegFailed {
                reason: format!("ffmpeg wait failed: {e}"),
            }
            .into());
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(EnvironmentError::FfmpegFailed {
                reason: "ffmpeg hung after decode pipes closed".into(),
            }
            .into());
        }
    };
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let reason = stderr.trim();
        let short = reason
            .lines()
            .last()
            .unwrap_or("ffmpeg failed")
            .chars()
            .take(400)
            .collect::<String>();
        return Err(UserError::InvalidAudio { reason: short }.into());
    }
    if samples_vec.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "ffmpeg produced no audio data (empty or corrupt file?)".into(),
        }
        .into());
    }
    let samples: Arc<[f32]> = samples_vec.into();
    let duration_secs = samples.len() as f64 / 16_000.0;
    if duration_secs + 0.05 >= max_duration_secs {
        return Err(UserError::AudioTooLong {
            duration_secs: max_duration_secs,
            max_secs: max_duration_secs,
        }
        .into());
    }
    if samples.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "audio contains no samples".into(),
        }
        .into());
    }
    Ok(AudioInput::from_parts_unchecked(
        path.to_path_buf(),
        samples,
        16_000,
        duration_secs,
    ))
}

/// Test helper: race cancel against stalled pipe reads on an arbitrary child
/// (JOE-1648 fourth-pass). Production decode uses the same `select!` pattern.
#[cfg(test)]
async fn race_cancel_against_stalled_pipes(
    child: &mut tokio::process::Child,
    cancel: crate::cancel::CancelFlag,
    poll_ms: u64,
) -> Result<()> {
    let mut stdout = child.stdout.take().ok_or_else(|| EnvironmentError::Other {
        message: "stdout missing".into(),
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| EnvironmentError::Other {
        message: "stderr missing".into(),
    })?;

    let stdout_task = async {
        let mut buf = [0u8; 1024];
        loop {
            let n = stdout
                .read(&mut buf)
                .await
                .map_err(|e| EnvironmentError::Other {
                    message: format!("stdout read: {e}"),
                })?;
            if n == 0 {
                break;
            }
        }
        Ok::<(), crate::error::TranscriptionError>(())
    };
    let stderr_task = async {
        let mut buf = [0u8; 1024];
        loop {
            let n = stderr
                .read(&mut buf)
                .await
                .map_err(|e| EnvironmentError::Other {
                    message: format!("stderr read: {e}"),
                })?;
            if n == 0 {
                break;
            }
        }
        Ok::<(), crate::error::TranscriptionError>(())
    };

    let drains = async { tokio::try_join!(stdout_task, stderr_task) };
    let flag = cancel.clone();
    let cancel_watch = async move {
        loop {
            if flag.is_cancelled() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
        }
    };

    tokio::select! {
        biased;
        _ = cancel_watch => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(crate::error::ProviderError::Cancelled.into())
        }
        r = drains => {
            let _ = child.wait().await;
            r.map(|_| ())
        }
    }
}

/// Write samples out as a 16 kHz mono WAV using an exclusive create (O_EXCL).
pub fn write_temp_wav(samples: &[f32], dest: &Path) -> Result<()> {
    // Never follow symlinks: require create_new. Callers that need overwrite must unlink first.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
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

/// Encode samples to a compressed temp file for remote upload (JOE-1648).
///
/// Returns `(path, format)` where format is `"mp3"` or `"wav"`.
/// Caller must delete `path` when done.
///
/// MP3 encoding uses the same supervised FFmpeg lifecycle as decode: `-nostdin`,
/// concurrent stderr drain, wall-clock deadline, kill+reap on failure. Destination
/// is exclusively created (no `-y` clobber).
///
/// WAV fallback is **only** for encoder/codec conversion failures (missing
/// libmp3lame, non-zero FFmpeg exit for encode reasons). Cancellation, absolute
/// deadline expiry, and size-cap violations **never** fall back to a successful
/// WAV (JOE-1648 third-pass residual).
pub async fn encode_for_upload(
    samples: &[f32],
    max_bytes: usize,
) -> Result<(PathBuf, &'static str)> {
    encode_for_upload_with_timeout(samples, max_bytes, DEFAULT_FFMPEG_TIMEOUT, None).await
}

/// Supervised upload encode with explicit deadline and optional cancel flag.
pub async fn encode_for_upload_with_timeout(
    samples: &[f32],
    max_bytes: usize,
    timeout: std::time::Duration,
    cancel: Option<crate::cancel::CancelFlag>,
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
        if let Some(flag) = &cancel {
            if flag.is_cancelled() {
                let _ = std::fs::remove_file(&wav_path);
                return Err(crate::error::ProviderError::Cancelled.into());
            }
        }

        // Reserve a unique path (O_EXCL), then remove so FFmpeg creates the file
        // without `-y` clobber semantics. `-n` refuses to overwrite if something
        // else appears at the path (JOE-1648).
        let mp3_path = {
            let t = tempfile::Builder::new()
                .prefix("aurum-upload-")
                .suffix(".mp3")
                .tempfile()
                .map_err(|e| EnvironmentError::Other {
                    message: format!("temp mp3: {e}"),
                })?;
            let p = t.path().to_path_buf();
            drop(t);
            let _ = std::fs::remove_file(&p);
            p
        };

        let encode = supervise_ffmpeg_encode(
            &ffmpeg,
            &wav_path,
            &mp3_path,
            max_bytes,
            timeout,
            cancel.clone(),
        )
        .await;

        match encode {
            Ok(()) => {
                let _ = std::fs::remove_file(&wav_path);
                if let Ok(meta) = std::fs::metadata(&mp3_path) {
                    if meta.len() > 0 && (meta.len() as usize) <= max_bytes {
                        return Ok((mp3_path, "mp3"));
                    }
                }
                let _ = std::fs::remove_file(&mp3_path);
                // Empty / oversize output is a conversion failure → WAV fallback.
            }
            Err(e) if is_terminal_upload_control_error(&e) => {
                let _ = std::fs::remove_file(&mp3_path);
                let _ = std::fs::remove_file(&wav_path);
                return Err(e);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&mp3_path);
                tracing::debug!(
                    error = %e,
                    "supervised mp3 encode failed with codec/conversion error; falling back to wav"
                );
                let _ = std::fs::remove_file(&wav_path);
            }
        }

        // Re-check control plane before returning fallback success.
        if let Some(flag) = &cancel {
            if flag.is_cancelled() {
                return Err(crate::error::ProviderError::Cancelled.into());
            }
        }
        write_temp_wav(samples, &wav_path)?;
    }

    if let Some(flag) = &cancel {
        if flag.is_cancelled() {
            let _ = std::fs::remove_file(&wav_path);
            return Err(crate::error::ProviderError::Cancelled.into());
        }
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

/// Errors that must not be rewritten as a successful WAV fallback.
fn is_terminal_upload_control_error(e: &crate::error::TranscriptionError) -> bool {
    use crate::error::{EnvironmentError, ProviderError, TranscriptionError, UserError};
    match e {
        TranscriptionError::Provider(ProviderError::Cancelled)
        | TranscriptionError::Provider(ProviderError::DeadlineExceeded)
        | TranscriptionError::User(UserError::AudioTooLarge { .. }) => true,
        TranscriptionError::Environment(EnvironmentError::FfmpegFailed { reason }) => {
            // Supervisor maps wall-clock timeout into this variant.
            reason.contains("wall-clock deadline") || reason.contains("deadline exceeded")
        }
        _ => false,
    }
}

/// Run FFmpeg encode with concurrent stderr drain, deadline, cancel, and file-size cap.
async fn supervise_ffmpeg_encode(
    ffmpeg: &Path,
    wav_path: &Path,
    mp3_path: &Path,
    max_bytes: usize,
    timeout: std::time::Duration,
    cancel: Option<crate::cancel::CancelFlag>,
) -> Result<()> {
    let mut child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-n", // never overwrite an unexpected existing path
            "-protocol_whitelist",
            "file,crypto,data",
            "-i",
        ])
        .arg(wav_path)
        .args(["-codec:a", "libmp3lame", "-b:a", "64k"])
        .arg(mp3_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| EnvironmentError::FfmpegFailed {
            reason: format!("failed to spawn ffmpeg encode: {e}"),
        })?;

    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| EnvironmentError::FfmpegFailed {
            reason: "ffmpeg stderr missing".into(),
        })?;

    // Concurrent stderr drain from process start (JOE-1648). Never wait for the
    // child first — a full stderr pipe would deadlock FFmpeg before exit.
    let cancel_stderr = cancel.clone();
    let stderr_join = tokio::spawn(async move {
        let mut tail: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4 * 1024];
        loop {
            if let Some(flag) = &cancel_stderr {
                if flag.is_cancelled() {
                    return Err(crate::error::ProviderError::Cancelled.into());
                }
            }
            let n = stderr
                .read(&mut buf)
                .await
                .map_err(|e| EnvironmentError::FfmpegFailed {
                    reason: format!("reading ffmpeg stderr: {e}"),
                })?;
            if n == 0 {
                break;
            }
            if tail.len() + n > STDERR_TAIL_CAP {
                let drop_n = (tail.len() + n).saturating_sub(STDERR_TAIL_CAP);
                if drop_n < tail.len() {
                    tail.drain(..drop_n);
                } else {
                    tail.clear();
                }
            }
            tail.extend_from_slice(&buf[..n]);
        }
        Ok::<Vec<u8>, crate::error::TranscriptionError>(tail)
    });

    // Manual deadline so we keep ownership of `child` for explicit kill/reap.
    let deadline = tokio::time::Instant::now() + timeout;
    let result: crate::error::Result<(std::process::ExitStatus, Vec<u8>)> = loop {
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill().await;
            let _ = child.wait().await;
            stderr_join.abort();
            let _ = stderr_join.await;
            // Distinct from codec failure so upload fallback cannot swallow it.
            break Err(crate::error::ProviderError::DeadlineExceeded.into());
        }
        if let Some(flag) = &cancel {
            if flag.is_cancelled() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_join.abort();
                let _ = stderr_join.await;
                break Err(crate::error::ProviderError::Cancelled.into());
            }
        }
        if let Ok(meta) = std::fs::metadata(mp3_path) {
            if meta.len() as usize > max_bytes {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_join.abort();
                let _ = stderr_join.await;
                break Err(UserError::AudioTooLarge {
                    decoded_bytes: meta.len() as usize,
                    max_bytes,
                }
                .into());
            }
        }
        // Poll child without blocking the concurrent stderr drain task.
        match child.try_wait() {
            Ok(Some(status)) => {
                let tail = match stderr_join.await {
                    Ok(Ok(t)) => t,
                    Ok(Err(e)) => break Err(e),
                    Err(e) => {
                        break Err(EnvironmentError::FfmpegFailed {
                            reason: format!("stderr join: {e}"),
                        }
                        .into());
                    }
                };
                break Ok((status, tail));
            }
            Ok(None) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_join.abort();
                let _ = stderr_join.await;
                break Err(EnvironmentError::FfmpegFailed {
                    reason: format!("ffmpeg encode wait failed: {e}"),
                }
                .into());
            }
        }
    };

    match result {
        Ok((status, tail)) => {
            if !status.success() {
                let diag = String::from_utf8_lossy(&tail);
                let short = diag
                    .lines()
                    .last()
                    .unwrap_or("ffmpeg encode failed")
                    .chars()
                    .take(400)
                    .collect::<String>();
                return Err(EnvironmentError::FfmpegFailed {
                    reason: format!("ffmpeg encode exited with {status}: {short}"),
                }
                .into());
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
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
        assert_eq!(audio.sample_rate(), 16_000);
        assert!(audio.samples().len() > 1000);
        assert!((audio.duration_secs() - 0.25).abs() < 0.01);
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
        assert_eq!(audio.samples().len(), samples.len());
    }

    #[test]
    fn from_pcm_basic() {
        let audio = AudioInput::from_pcm_slice(&[0.0; 3200], WHISPER_SAMPLE_RATE).unwrap();
        assert_eq!(audio.len(), 3200);
        assert!((audio.duration_secs() - 0.2).abs() < 1e-9);
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

    #[test]
    fn upload_control_errors_are_not_fallback_eligible() {
        use crate::error::{EnvironmentError, ProviderError, TranscriptionError, UserError};
        assert!(is_terminal_upload_control_error(
            &TranscriptionError::Provider(ProviderError::Cancelled)
        ));
        assert!(is_terminal_upload_control_error(
            &TranscriptionError::Provider(ProviderError::DeadlineExceeded)
        ));
        assert!(is_terminal_upload_control_error(&TranscriptionError::User(
            UserError::AudioTooLarge {
                decoded_bytes: 9,
                max_bytes: 1,
            }
        )));
        assert!(is_terminal_upload_control_error(
            &TranscriptionError::Environment(EnvironmentError::FfmpegFailed {
                reason: "ffmpeg encode exceeded wall-clock deadline (1s)".into(),
            })
        ));
        // Codec/non-zero exit may fall back to WAV.
        assert!(!is_terminal_upload_control_error(
            &TranscriptionError::Environment(EnvironmentError::FfmpegFailed {
                reason: "ffmpeg encode exited with exit status: 1: Unknown encoder".into(),
            })
        ));
        assert!(!is_terminal_upload_control_error(
            &TranscriptionError::Environment(EnvironmentError::FfmpegMissing)
        ));
    }

    #[tokio::test]
    async fn encode_for_upload_pre_cancel_does_not_succeed() {
        let cancel = crate::cancel::CancelFlag::new();
        cancel.cancel();
        let samples = vec![0.0f32; 1600];
        let err = encode_for_upload_with_timeout(
            &samples,
            DEFAULT_MAX_UPLOAD_BYTES,
            std::time::Duration::from_secs(5),
            Some(cancel),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(crate::error::ProviderError::Cancelled)
        ));
    }

    /// Stalled pipe reads must not delay cancellation until a long timeout
    /// (JOE-1648 fourth-pass). `sleep` keeps stdout/stderr open without data.
    #[tokio::test]
    async fn stalled_pipe_cancel_kills_child_promptly() {
        use std::process::Stdio;
        use std::time::Instant;
        use tokio::process::Command;

        let mut child = Command::new("sleep")
            .arg("60")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn sleep");
        let cancel = crate::cancel::CancelFlag::new();
        let cancel_for_task = cancel.clone();
        let start = Instant::now();
        // Start drains first so reads park on empty pipes, then cancel.
        let join = tokio::spawn(async move {
            race_cancel_against_stalled_pipes(&mut child, cancel_for_task, 15).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel.cancel();
        let err = join.await.expect("join").unwrap_err();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "cancel must not wait for long timeout; elapsed {:?}",
            start.elapsed()
        );
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(crate::error::ProviderError::Cancelled)
        ));
    }
}
