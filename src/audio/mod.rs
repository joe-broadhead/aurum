//! Audio loading and conversion.
//!
//! Strategy for v0.0.0:
//! - Prefer system `ffmpeg` for decoding any common format to 16 kHz mono f32 PCM
//! - Fail fast with install instructions if ffmpeg is missing
//! - WAV files that are already 16 kHz mono PCM can be read directly via `hound`
//!   as a fast path (still requires ffmpeg for other formats / conversions)

use crate::error::{EnvironmentError, Result, UserError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use which::which;

/// Supported input extensions (informational; ffmpeg is the real gate).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "wav", "flac", "ogg", "opus", "webm", "mp4", "aac", "wma", "mkv",
];

/// In-memory audio ready for a transcription provider.
#[derive(Debug, Clone)]
pub struct AudioInput {
    /// Original file path (for display / remote upload).
    pub source_path: PathBuf,
    /// 16 kHz mono f32 samples in [-1.0, 1.0].
    pub samples: Vec<f32>,
    /// Sample rate — always 16_000 after conversion.
    pub sample_rate: u32,
    /// Duration in seconds.
    pub duration_secs: f64,
}

impl AudioInput {
    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
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
    if !path.exists() {
        return Err(UserError::FileNotFound {
            path: path.display().to_string(),
        }
        .into());
    }

    // Fast path: already-correct WAV.
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        if let Ok(audio) = try_load_wav_direct(path) {
            return Ok(audio);
        }
        // Fall through to ffmpeg if the WAV isn't in the expected format.
        tracing::debug!("WAV direct-load failed; falling back to ffmpeg");
    }

    load_via_ffmpeg(path).await
}

/// Attempt to read a 16 kHz mono PCM WAV directly.
fn try_load_wav_direct(path: &Path) -> Result<AudioInput> {
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
            // Convert f32 WAV to i16 range then back — simpler to reject and use ffmpeg.
            return Err(UserError::InvalidAudio {
                reason: "float WAV; use ffmpeg path".into(),
            }
            .into());
        }
    };

    let samples_i16 = samples_i16.map_err(|e| UserError::InvalidAudio {
        reason: format!("failed reading samples: {e}"),
    })?;

    let samples: Vec<f32> = samples_i16
        .iter()
        .map(|s| *s as f32 / i16::MAX as f32)
        .collect();

    let duration_secs = samples.len() as f64 / 16_000.0;

    Ok(AudioInput {
        source_path: path.to_path_buf(),
        samples,
        sample_rate: 16_000,
        duration_secs,
    })
}

/// Decode any format to 16 kHz mono f32 via ffmpeg (s16le intermediate).
async fn load_via_ffmpeg(path: &Path) -> Result<AudioInput> {
    let ffmpeg = require_ffmpeg()?;

    let output = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
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
        .output()
        .await
        .map_err(|e| EnvironmentError::FfmpegFailed {
            reason: format!("failed to spawn ffmpeg: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(
            if stderr.to_lowercase().contains("invalid data")
                || stderr.to_lowercase().contains("could not find codec")
                || stderr.to_lowercase().contains("error opening")
            {
                UserError::InvalidAudio {
                    reason: stderr.trim().to_string(),
                }
                .into()
            } else {
                EnvironmentError::FfmpegFailed {
                    reason: stderr.trim().to_string(),
                }
                .into()
            },
        );
    }

    if output.stdout.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: "ffmpeg produced no audio data (empty or corrupt file?)".into(),
        }
        .into());
    }

    if output.stdout.len() % 2 != 0 {
        return Err(UserError::InvalidAudio {
            reason: "ffmpeg produced misaligned PCM data".into(),
        }
        .into());
    }

    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(2)
        .map(|c| {
            let s = i16::from_le_bytes([c[0], c[1]]);
            s as f32 / i16::MAX as f32
        })
        .collect();

    let duration_secs = samples.len() as f64 / 16_000.0;

    Ok(AudioInput {
        source_path: path.to_path_buf(),
        samples,
        sample_rate: 16_000,
        duration_secs,
    })
}

/// Write samples back out as a temporary 16 kHz mono WAV (used by remote providers).
pub fn write_temp_wav(samples: &[f32], dest: &Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(dest, spec).map_err(|e| EnvironmentError::Other {
        message: format!("failed to create temp wav {}: {e}", dest.display()),
    })?;

    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i = (clamped * i16::MAX as f32) as i16;
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
            w.write_sample((sample * i16::MAX as f32 * 0.2) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn loads_16k_mono_wav_direct() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        synthesize_sine_wav(&path, 0.25, 440.0);

        let audio = try_load_wav_direct(&path).unwrap();
        assert_eq!(audio.sample_rate, 16_000);
        assert!(audio.samples.len() > 1000);
        assert!((audio.duration_secs - 0.25).abs() < 0.01);
    }

    #[test]
    fn missing_file_errors() {
        let err = load_audio_blocking(Path::new("/no/such/file.wav"));
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
        let audio = try_load_wav_direct(&path).unwrap();
        assert_eq!(audio.samples.len(), samples.len());
    }
}
