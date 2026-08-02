//! Bounded remote-audio normalization for remote TTS (JOE-1937).
//!
//! Converts provider wire bytes into validated mono `i16` PCM plus an accurate
//! sample rate. Prefer in-process PCM/WAV paths; use supervised FFmpeg only for
//! compressed containers (MP3). Provider bytes never bypass encoded/decoded caps.
//!
//! Security: remote audio is untrusted parser input. Error messages never include
//! audio body bytes, PCM previews, or synthesis text.

use crate::error::{EnvironmentError, ProviderError, Result};
use crate::runtime::OpContext;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Default hard cap on encoded provider response bodies (16 MiB).
pub const DEFAULT_MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;

/// Default hard cap on decoded mono PCM sample count (~10 min @ 48 kHz).
pub const DEFAULT_MAX_PCM_SAMPLES: usize = 48_000 * 600;

/// Default maximum duration of normalized audio.
pub const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(600);

/// Discrete sample rates accepted for remote TTS wire formats.
pub const ALLOWED_SAMPLE_RATES_HZ: &[u32] = &[
    8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000,
];

/// Wire format declared by the provider/capability path (not raw MIME guessing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedAudioFormat {
    /// Little-endian signed 16-bit PCM with an explicit layout.
    PcmS16Le { sample_rate_hz: u32, channels: u16 },
    /// RIFF WAVE container (in-process bounded parser when PCM-compatible).
    Wav,
    /// MPEG-1/2 Layer III (supervised FFmpeg → mono WAV → in-process parse).
    Mp3,
}

impl EncodedAudioFormat {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::PcmS16Le { .. } => "pcm_s16le",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
        }
    }
}

/// How multi-channel PCM is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelPolicy {
    /// Reject any channel count other than 1.
    MonoOnly,
    /// Average L+R for stereo (deterministic); reject >2 channels.
    #[default]
    DownmixStereo,
}

/// Resource bounds for remote audio normalization.
#[derive(Debug, Clone, Copy)]
pub struct RemoteAudioLimits {
    pub max_encoded_bytes: usize,
    pub max_pcm_samples: usize,
    pub max_duration: Duration,
    pub channel_policy: ChannelPolicy,
}

impl Default for RemoteAudioLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
            max_pcm_samples: DEFAULT_MAX_PCM_SAMPLES,
            max_duration: DEFAULT_MAX_DURATION,
            channel_policy: ChannelPolicy::DownmixStereo,
        }
    }
}

impl RemoteAudioLimits {
    /// Tight limits for unit tests and fuzz budgets.
    pub fn tight() -> Self {
        Self {
            max_encoded_bytes: 64 * 1024,
            max_pcm_samples: 48_000, // 1 s @ 48 kHz
            max_duration: Duration::from_secs(2),
            channel_policy: ChannelPolicy::DownmixStereo,
        }
    }

    pub fn sample_rate_allowed(self, rate: u32) -> bool {
        ALLOWED_SAMPLE_RATES_HZ.contains(&rate)
    }
}

/// Encoded provider body already under a hard byte cap.
#[derive(Debug, Clone)]
pub struct BoundedAudioBody {
    bytes: Vec<u8>,
}

impl BoundedAudioBody {
    /// Construct from fully buffered bytes, enforcing `max_bytes`.
    ///
    /// Prefer streaming caps ([`crate::remote::read_body_limited`]) at the HTTP
    /// layer; this is a second gate before decode.
    pub fn try_from_bytes(bytes: Vec<u8>, max_bytes: usize, provider: &str) -> Result<Self> {
        if bytes.len() > max_bytes {
            return Err(ProviderError::ResponseTooLarge {
                provider: provider.into(),
                reason: format!(
                    "encoded audio body {} bytes exceeds cap {max_bytes}",
                    bytes.len()
                ),
            }
            .into());
        }
        if bytes.is_empty() {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: "encoded audio body is empty".into(),
            }
            .into());
        }
        Ok(Self { bytes })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Validated mono PCM result of remote-audio normalization.
#[derive(Debug, Clone)]
pub struct NormalizedAudio {
    pub pcm_i16_mono: Vec<i16>,
    pub sample_rate_hz: u32,
    /// Duration derived from final PCM length and sample rate.
    pub duration_ms: u64,
    /// Wire format that produced this PCM (for honesty / diagnostics).
    pub source_format: &'static str,
}

impl NormalizedAudio {
    pub fn channels(&self) -> u16 {
        1
    }

    pub fn sample_count(&self) -> usize {
        self.pcm_i16_mono.len()
    }
}

/// Normalize provider audio bytes into mono `i16` PCM under shared limits.
///
/// `format` must come from capability/request context — MIME headers alone are
/// not trusted. Format mismatches fail with [`ProviderError::InvalidProviderPayload`].
pub async fn normalize_remote_audio(
    body: BoundedAudioBody,
    format: EncodedAudioFormat,
    limits: RemoteAudioLimits,
    op: &OpContext,
    provider: &str,
) -> Result<NormalizedAudio> {
    op.check()?;
    if body.len() > limits.max_encoded_bytes {
        return Err(ProviderError::ResponseTooLarge {
            provider: provider.into(),
            reason: format!(
                "encoded audio body {} bytes exceeds cap {}",
                body.len(),
                limits.max_encoded_bytes
            ),
        }
        .into());
    }

    match format {
        EncodedAudioFormat::PcmS16Le {
            sample_rate_hz,
            channels,
        } => {
            let pcm =
                decode_pcm_s16le(body.as_slice(), sample_rate_hz, channels, limits, provider)?;
            op.check()?;
            finalize_mono(pcm, sample_rate_hz, limits, provider, format.as_label())
        }
        EncodedAudioFormat::Wav => {
            let (pcm, rate) = decode_wav_in_process(body.as_slice(), limits, provider)?;
            op.check()?;
            finalize_mono(pcm, rate, limits, provider, format.as_label())
        }
        EncodedAudioFormat::Mp3 => {
            let (pcm, rate) = decode_mp3_supervised(body.as_slice(), limits, op, provider).await?;
            op.check()?;
            finalize_mono(pcm, rate, limits, provider, format.as_label())
        }
    }
}

fn finalize_mono(
    pcm: Vec<i16>,
    sample_rate_hz: u32,
    limits: RemoteAudioLimits,
    provider: &str,
    source_format: &'static str,
) -> Result<NormalizedAudio> {
    validate_sample_rate(sample_rate_hz, limits, provider)?;
    if pcm.is_empty() {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "decoded audio contains no samples".into(),
        }
        .into());
    }
    if pcm.len() > limits.max_pcm_samples {
        return Err(ProviderError::LimitExceeded {
            reason: format!(
                "decoded PCM has {} samples (limit {})",
                pcm.len(),
                limits.max_pcm_samples
            ),
        }
        .into());
    }
    let duration_ms = duration_ms_from_pcm(pcm.len(), sample_rate_hz);
    let max_ms = limits.max_duration.as_millis() as u64;
    if duration_ms > max_ms {
        return Err(ProviderError::LimitExceeded {
            reason: format!("decoded audio duration {duration_ms} ms exceeds limit {max_ms} ms"),
        }
        .into());
    }
    // Minimum ~5 ms (match local TTS soft floor intent).
    let min_samples = (sample_rate_hz as usize / 200).max(1);
    if pcm.len() < min_samples {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!(
                "decoded audio too short ({} samples at {sample_rate_hz} Hz)",
                pcm.len()
            ),
        }
        .into());
    }
    Ok(NormalizedAudio {
        pcm_i16_mono: pcm,
        sample_rate_hz,
        duration_ms,
        source_format,
    })
}

fn duration_ms_from_pcm(sample_count: usize, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    (sample_count as u64)
        .saturating_mul(1000)
        .checked_div(sample_rate_hz as u64)
        .unwrap_or(0)
}

fn validate_sample_rate(rate: u32, limits: RemoteAudioLimits, provider: &str) -> Result<()> {
    if rate == 0 {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "sample rate is zero".into(),
        }
        .into());
    }
    if !limits.sample_rate_allowed(rate) {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!("sample rate {rate} Hz is not in the allowed remote set"),
        }
        .into());
    }
    Ok(())
}

fn decode_pcm_s16le(
    bytes: &[u8],
    sample_rate_hz: u32,
    channels: u16,
    limits: RemoteAudioLimits,
    provider: &str,
) -> Result<Vec<i16>> {
    validate_sample_rate(sample_rate_hz, limits, provider)?;
    if channels == 0 {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "PCM channel count is zero".into(),
        }
        .into());
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "PCM body has odd byte count (not s16le-aligned)".into(),
        }
        .into());
    }
    let frame_bytes = 2usize.saturating_mul(channels as usize);
    if frame_bytes == 0 || !bytes.len().is_multiple_of(frame_bytes) {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "PCM body length is not an integer number of frames".into(),
        }
        .into());
    }

    let frame_count = bytes.len() / frame_bytes;
    // Pre-check mono sample budget after downmix.
    if frame_count > limits.max_pcm_samples {
        return Err(ProviderError::LimitExceeded {
            reason: format!(
                "PCM frame count {frame_count} exceeds sample cap {}",
                limits.max_pcm_samples
            ),
        }
        .into());
    }

    let interleaved = read_i16le_samples(bytes);
    apply_channel_policy(&interleaved, channels, limits.channel_policy, provider)
}

fn read_i16le_samples(bytes: &[u8]) -> Vec<i16> {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    out
}

fn apply_channel_policy(
    interleaved: &[i16],
    channels: u16,
    policy: ChannelPolicy,
    provider: &str,
) -> Result<Vec<i16>> {
    match (channels, policy) {
        (1, _) => Ok(interleaved.to_vec()),
        (2, ChannelPolicy::DownmixStereo) => {
            if !interleaved.len().is_multiple_of(2) {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "stereo PCM sample count is not even".into(),
                }
                .into());
            }
            let mut mono = Vec::with_capacity(interleaved.len() / 2);
            for pair in interleaved.chunks_exact(2) {
                // Deterministic average with rounding toward zero via i32 mid.
                let l = pair[0] as i32;
                let r = pair[1] as i32;
                mono.push(((l + r) / 2) as i16);
            }
            Ok(mono)
        }
        (2, ChannelPolicy::MonoOnly) => Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "stereo PCM rejected by mono-only channel policy".into(),
        }
        .into()),
        (n, _) => Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!("{n}-channel PCM is not supported (max 2 with downmix)"),
        }
        .into()),
    }
}

fn decode_wav_in_process(
    bytes: &[u8],
    limits: RemoteAudioLimits,
    provider: &str,
) -> Result<(Vec<i16>, u32)> {
    // Quick RIFF magic check — fail closed without guessing MP3-as-WAV.
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "body is not a RIFF/WAVE container (format/capability mismatch)".into(),
        }
        .into());
    }

    let cursor = Cursor::new(bytes);
    let reader =
        hound::WavReader::new(cursor).map_err(|_| ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "malformed WAV header or chunks".into(),
        })?;
    let spec = reader.spec();

    if spec.sample_rate == 0 || spec.channels == 0 {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "WAV declares zero sample rate or channels".into(),
        }
        .into());
    }
    validate_sample_rate(spec.sample_rate, limits, provider)?;

    if spec.sample_format != hound::SampleFormat::Int {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "float WAV is not accepted on the remote in-process path".into(),
        }
        .into());
    }
    if spec.bits_per_sample != 16 {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!(
                "WAV bits_per_sample {} not supported (need 16)",
                spec.bits_per_sample
            ),
        }
        .into());
    }

    // Bound by declared duration before materializing full PCM.
    let declared = reader.duration() as usize; // frames
    if declared > limits.max_pcm_samples {
        return Err(ProviderError::LimitExceeded {
            reason: format!(
                "WAV declares {declared} frames exceeding sample cap {}",
                limits.max_pcm_samples
            ),
        }
        .into());
    }

    let mut interleaved: Vec<i16> = Vec::with_capacity(
        declared
            .saturating_mul(spec.channels as usize)
            .min(limits.max_pcm_samples.saturating_mul(2)),
    );
    for sample in reader.into_samples::<i16>() {
        let s = sample.map_err(|_| ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "failed reading WAV samples".into(),
        })?;
        let frames_so_far = interleaved.len() / (spec.channels as usize).max(1);
        if frames_so_far >= limits.max_pcm_samples {
            return Err(ProviderError::LimitExceeded {
                reason: format!("WAV sample stream exceeded cap {}", limits.max_pcm_samples),
            }
            .into());
        }
        interleaved.push(s);
    }

    let mono = apply_channel_policy(&interleaved, spec.channels, limits.channel_policy, provider)?;
    Ok((mono, spec.sample_rate))
}

/// Supervised FFmpeg decode of MP3 → mono WAV → in-process parser.
async fn decode_mp3_supervised(
    bytes: &[u8],
    limits: RemoteAudioLimits,
    op: &OpContext,
    provider: &str,
) -> Result<(Vec<i16>, u32)> {
    op.check()?;
    let ffmpeg = which::which("ffmpeg").map_err(|_| EnvironmentError::FfmpegMissing)?;

    let mp3_tmp = tempfile::Builder::new()
        .prefix("aurum-remote-")
        .suffix(".mp3")
        .tempfile()
        .map_err(|e| EnvironmentError::Other {
            message: format!("temp mp3: {e}"),
        })?;
    let mp3_path = mp3_tmp.path().to_path_buf();
    std::fs::write(&mp3_path, bytes).map_err(|e| EnvironmentError::Other {
        message: format!("write temp mp3: {e}"),
    })?;
    // Keep file until decode finishes; drop tempfile later.
    let _mp3_keep = mp3_tmp;

    let wav_tmp = tempfile::Builder::new()
        .prefix("aurum-remote-")
        .suffix(".wav")
        .tempfile()
        .map_err(|e| EnvironmentError::Other {
            message: format!("temp wav: {e}"),
        })?;
    let wav_path = wav_tmp.path().to_path_buf();
    drop(wav_tmp);
    let _ = std::fs::remove_file(&wav_path);

    // Cap duration via -t; keep native sample rate (no arbitrary -ar).
    let max_secs = limits.max_duration.as_secs_f64().max(0.001);
    let max_t = format!("{max_secs:.3}");
    // Encoded byte cap already applied; bound decoded WAV file size ~ 2 * max samples + header.
    let max_wav_bytes = limits
        .max_pcm_samples
        .saturating_mul(2)
        .saturating_add(4096);

    let timeout = op
        .remaining()
        .unwrap_or(Duration::from_secs(120))
        .min(Duration::from_secs(120));

    let result = run_ffmpeg_mp3_to_wav(
        &ffmpeg,
        &mp3_path,
        &wav_path,
        &max_t,
        max_wav_bytes,
        timeout,
        op,
        provider,
    )
    .await;

    let _ = std::fs::remove_file(&mp3_path);
    let decode = result;
    let out = match decode {
        Ok(()) => {
            let wav_bytes = std::fs::read(&wav_path).map_err(|e| EnvironmentError::Other {
                message: format!("read decoded wav: {e}"),
            })?;
            let _ = std::fs::remove_file(&wav_path);
            if wav_bytes.len() > max_wav_bytes {
                return Err(ProviderError::LimitExceeded {
                    reason: format!(
                        "decoded WAV size {} exceeds bound {max_wav_bytes}",
                        wav_bytes.len()
                    ),
                }
                .into());
            }
            if wav_bytes.len() > limits.max_encoded_bytes.saturating_mul(32) {
                // Separate decompression-amplification guard (encoded → wav).
                return Err(ProviderError::LimitExceeded {
                    reason: "decoded WAV exceeds decompression amplification bound".into(),
                }
                .into());
            }
            decode_wav_in_process(&wav_bytes, limits, provider)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&wav_path);
            Err(e)
        }
    };
    out
}

#[allow(clippy::too_many_arguments)]
async fn run_ffmpeg_mp3_to_wav(
    ffmpeg: &std::path::Path,
    mp3_path: &PathBuf,
    wav_path: &PathBuf,
    max_t: &str,
    max_wav_bytes: usize,
    timeout: Duration,
    op: &OpContext,
    provider: &str,
) -> Result<()> {
    op.check()?;
    let protocol_whitelist = "file,crypto,data";

    let mut child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            protocol_whitelist,
            "-i",
        ])
        .arg(mp3_path)
        .args(["-t", max_t, "-ac", "1", "-f", "wav", "-acodec", "pcm_s16le"])
        .arg(wav_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| EnvironmentError::FfmpegFailed {
            reason: format!("failed to spawn ffmpeg: {e}"),
        })?;

    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| EnvironmentError::FfmpegFailed {
            reason: "ffmpeg stderr missing".into(),
        })?;

    const STDERR_TAIL_CAP: usize = 8 * 1024;
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

    let cancel = op.cancel.clone();
    let cancel_watch = async move {
        loop {
            if cancel.is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };

    let drain_outcome: Result<Vec<u8>> = tokio::select! {
        biased;
        _ = cancel_watch => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ProviderError::Cancelled.into())
        }
        timed = tokio::time::timeout(timeout, stderr_task) => {
            match timed {
                Ok(Ok(tail)) => Ok(tail),
                Ok(Err(e)) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(e)
                }
                Err(_elapsed) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(ProviderError::DeadlineExceeded.into())
                }
            }
        }
    };

    let stderr_bytes = drain_outcome?;

    let status = match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
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
                reason: "ffmpeg hung after decode".into(),
            }
            .into());
        }
    };

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let short = stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("ffmpeg failed")
            .chars()
            .take(200)
            .collect::<String>();
        // Do not include path or body; map as format/payload failure when possible.
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!("MP3 decode failed: {short}"),
        }
        .into());
    }

    if let Ok(meta) = std::fs::metadata(wav_path) {
        if meta.len() as usize > max_wav_bytes {
            let _ = std::fs::remove_file(wav_path);
            return Err(ProviderError::LimitExceeded {
                reason: format!(
                    "decoded WAV file {} bytes exceeds bound {max_wav_bytes}",
                    meta.len()
                ),
            }
            .into());
        }
        if meta.len() == 0 {
            return Err(ProviderError::InvalidProviderPayload {
                provider: provider.into(),
                reason: "MP3 decode produced empty audio".into(),
            }
            .into());
        }
    } else {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "MP3 decode produced no output file".into(),
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::OpContext;
    use std::io::Cursor;

    fn provider() -> &'static str {
        "test-remote"
    }

    fn write_wav_i16(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for &s in samples {
                w.write_sample(s).unwrap();
            }
            w.finalize().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn bounded_body_rejects_oversize_and_empty() {
        let err = BoundedAudioBody::try_from_bytes(vec![0; 10], 5, provider()).unwrap_err();
        assert!(err.to_string().contains("exceeds") || err.to_string().contains("large"));
        let err = BoundedAudioBody::try_from_bytes(vec![], 100, provider()).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn pcm_rejects_odd_byte_count() {
        let limits = RemoteAudioLimits::tight();
        let err = decode_pcm_s16le(&[0, 1, 2], 24_000, 1, limits, provider()).unwrap_err();
        assert!(err.to_string().contains("odd") || err.to_string().contains("align"));
    }

    #[test]
    fn pcm_rejects_zero_rate_and_channels() {
        let limits = RemoteAudioLimits::tight();
        assert!(decode_pcm_s16le(&[0, 0], 0, 1, limits, provider()).is_err());
        assert!(decode_pcm_s16le(&[0, 0], 24_000, 0, limits, provider()).is_err());
    }

    #[test]
    fn pcm_rejects_disallowed_rate() {
        let limits = RemoteAudioLimits::tight();
        let err = decode_pcm_s16le(&[0, 0, 0, 0], 13_000, 1, limits, provider()).unwrap_err();
        assert!(err.to_string().contains("sample rate"));
    }

    #[test]
    fn pcm_mono_roundtrip() {
        let limits = RemoteAudioLimits::tight();
        // 100 ms @ 24 kHz = 2400 samples
        let samples: Vec<i16> = (0..2400).map(|i| (i % 100) as i16).collect();
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let out = decode_pcm_s16le(&bytes, 24_000, 1, limits, provider()).unwrap();
        assert_eq!(out, samples);
    }

    #[test]
    fn pcm_stereo_downmix_averages() {
        let limits = RemoteAudioLimits::tight();
        // frames: (100,200), (10,30) → 150, 20
        let bytes = {
            let mut b = Vec::new();
            for s in [100i16, 200, 10, 30] {
                b.extend_from_slice(&s.to_le_bytes());
            }
            b
        };
        // pad to min duration: need ~120 samples at 24k for 5ms
        let mut frames = bytes;
        for _ in 0..200 {
            frames.extend_from_slice(&0i16.to_le_bytes());
            frames.extend_from_slice(&0i16.to_le_bytes());
        }
        let out = decode_pcm_s16le(&frames, 24_000, 2, limits, provider()).unwrap();
        assert_eq!(out[0], 150);
        assert_eq!(out[1], 20);
    }

    #[test]
    fn pcm_stereo_mono_only_rejects() {
        let mut limits = RemoteAudioLimits::tight();
        limits.channel_policy = ChannelPolicy::MonoOnly;
        let bytes = [0u8; 8];
        let err = decode_pcm_s16le(&bytes, 24_000, 2, limits, provider()).unwrap_err();
        assert!(err.to_string().contains("mono-only") || err.to_string().contains("stereo"));
    }

    #[test]
    fn pcm_rejects_over_sample_cap() {
        let limits = RemoteAudioLimits {
            max_pcm_samples: 10,
            ..RemoteAudioLimits::tight()
        };
        let bytes = vec![0u8; 40]; // 20 mono samples
        let err = decode_pcm_s16le(&bytes, 24_000, 1, limits, provider()).unwrap_err();
        assert!(err.to_string().contains("cap") || err.to_string().contains("limit"));
    }

    #[tokio::test]
    async fn normalize_pcm_end_to_end() {
        let limits = RemoteAudioLimits::tight();
        let samples: Vec<i16> = (0..2400).map(|i| ((i % 50) as i16) * 10).collect();
        let mut bytes = Vec::new();
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let body =
            BoundedAudioBody::try_from_bytes(bytes, limits.max_encoded_bytes, provider()).unwrap();
        let op = OpContext::new();
        let norm = normalize_remote_audio(
            body,
            EncodedAudioFormat::PcmS16Le {
                sample_rate_hz: 24_000,
                channels: 1,
            },
            limits,
            &op,
            provider(),
        )
        .await
        .unwrap();
        assert_eq!(norm.sample_rate_hz, 24_000);
        assert_eq!(norm.pcm_i16_mono, samples);
        assert_eq!(norm.duration_ms, 100);
        assert_eq!(norm.source_format, "pcm_s16le");
        assert_eq!(norm.channels(), 1);
    }

    #[tokio::test]
    async fn normalize_wav_mono() {
        let limits = RemoteAudioLimits::tight();
        let samples: Vec<i16> = (0..2400).map(|i| (i % 100) as i16).collect();
        let wav = write_wav_i16(&samples, 24_000, 1);
        let body =
            BoundedAudioBody::try_from_bytes(wav, limits.max_encoded_bytes, provider()).unwrap();
        let norm = normalize_remote_audio(
            body,
            EncodedAudioFormat::Wav,
            limits,
            &OpContext::new(),
            provider(),
        )
        .await
        .unwrap();
        assert_eq!(norm.pcm_i16_mono, samples);
        assert_eq!(norm.sample_rate_hz, 24_000);
    }

    #[tokio::test]
    async fn normalize_wav_rejects_non_riff() {
        let limits = RemoteAudioLimits::tight();
        let body = BoundedAudioBody::try_from_bytes(
            b"not-a-wav-file-content-here!!!!".to_vec(),
            limits.max_encoded_bytes,
            provider(),
        )
        .unwrap();
        let err = normalize_remote_audio(
            body,
            EncodedAudioFormat::Wav,
            limits,
            &OpContext::new(),
            provider(),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("RIFF")
                || err.to_string().contains("WAVE")
                || err.to_string().contains("mismatch")
        );
    }

    #[tokio::test]
    async fn normalize_wav_rejects_oversized_declaration() {
        let limits = RemoteAudioLimits {
            max_pcm_samples: 100,
            ..RemoteAudioLimits::tight()
        };
        // 500 frames @ 24k — exceeds 100 sample cap
        let samples: Vec<i16> = vec![1; 500];
        let wav = write_wav_i16(&samples, 24_000, 1);
        let body =
            BoundedAudioBody::try_from_bytes(wav, limits.max_encoded_bytes, provider()).unwrap();
        let err = normalize_remote_audio(
            body,
            EncodedAudioFormat::Wav,
            limits,
            &OpContext::new(),
            provider(),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("cap")
                || err.to_string().contains("limit")
                || err.to_string().contains("exceed")
        );
    }

    #[tokio::test]
    async fn cancel_before_normalize() {
        let limits = RemoteAudioLimits::tight();
        let body =
            BoundedAudioBody::try_from_bytes(vec![0; 100], limits.max_encoded_bytes, provider())
                .unwrap();
        let op = OpContext::new();
        op.cancel.cancel();
        let err = normalize_remote_audio(
            body,
            EncodedAudioFormat::PcmS16Le {
                sample_rate_hz: 24_000,
                channels: 1,
            },
            limits,
            &op,
            provider(),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::Provider(ProviderError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn duration_matches_pcm_length() {
        let limits = RemoteAudioLimits::tight();
        // 500 ms @ 16 kHz
        let n = 8_000;
        let samples = vec![100i16; n];
        let mut bytes = Vec::new();
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let body =
            BoundedAudioBody::try_from_bytes(bytes, limits.max_encoded_bytes, provider()).unwrap();
        let norm = normalize_remote_audio(
            body,
            EncodedAudioFormat::PcmS16Le {
                sample_rate_hz: 16_000,
                channels: 1,
            },
            limits,
            &OpContext::new(),
            provider(),
        )
        .await
        .unwrap();
        assert_eq!(norm.duration_ms, 500);
    }
}
