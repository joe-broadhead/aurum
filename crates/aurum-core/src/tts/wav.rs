//! Mono PCM → WAV writer (in-process, no ffmpeg).

use crate::error::{EnvironmentError, Result};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

/// Scale/clip f32 PCM into i16, never producing NaN.
///
/// If peak exceeds `peak_limit` (e.g. 0.95), the whole buffer is scaled down.
pub(crate) fn peak_guard_f32_to_i16(samples: &[f32], peak_limit: f32) -> Vec<i16> {
    let peak = samples
        .iter()
        .map(|s| if s.is_finite() { s.abs() } else { 0.0 })
        .fold(0.0f32, f32::max);
    let scale = if peak > peak_limit && peak > 0.0 {
        peak_limit / peak
    } else {
        1.0
    };
    samples
        .iter()
        .map(|s| {
            let v = if s.is_finite() { *s * scale } else { 0.0 };
            let scaled = (v * 32767.0).round().clamp(-32768.0, 32767.0);
            scaled as i16
        })
        .collect()
}

/// Write mono i16 PCM to a WAV file (overwrite destination).
pub(crate) fn write_wav_i16_mono(path: &Path, samples: &[i16], sample_rate_hz: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| EnvironmentError::DirectoryAccess {
                path: parent.display().to_string(),
                reason: e.to_string(),
            })?;
        }
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let file = File::create(path).map_err(|e| EnvironmentError::DiskSpace {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let mut writer =
        hound::WavWriter::new(BufWriter::new(file), spec).map_err(|e| EnvironmentError::Other {
            message: format!("failed to create wav writer for {}: {e}", path.display()),
        })?;
    for s in samples {
        writer
            .write_sample(*s)
            .map_err(|e| EnvironmentError::DiskSpace {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
    }
    writer.finalize().map_err(|e| EnvironmentError::Other {
        message: format!("failed to finalize wav {}: {e}", path.display()),
    })?;
    Ok(())
}

/// Atomic write: temp file in the same directory → rename.
pub fn write_wav_i16_mono_atomic(path: &Path, samples: &[i16], sample_rate_hz: u32) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|e| EnvironmentError::DirectoryAccess {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }
    let tmp_name = format!(
        ".{}.{}.partial.wav",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("out"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);
    let result = write_wav_i16_mono(&tmp_path, samples, sample_rate_hz);
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    // Windows cannot rename over an existing destination. Callers authorize
    // overwrite via check_overwrite(--force); remove dest then rename.
    replace_file(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        EnvironmentError::DirectoryAccess {
            path: path.display().to_string(),
            reason: format!("atomic replace failed: {e}"),
        }
        .into()
    })
}

/// Same-directory temp → destination replace (portable across Unix/Windows).
fn replace_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    fs::rename(tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn peak_guard_clips_nan() {
        let pcm = peak_guard_f32_to_i16(&[0.5, f32::NAN, 2.0, f32::NEG_INFINITY], 0.95);
        assert_eq!(pcm.len(), 4);
        assert_eq!(pcm[1], 0);
        // peak was 2.0 → scale 0.95/2 so sample is reduced vs unscaled 2.0
        assert!(pcm[2] != 0);
    }

    #[test]
    fn wav_header_basics() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples = vec![0i16, 1000, -1000, 0];
        write_wav_i16_mono_atomic(&path, &samples, 24_000).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 44);
        let mut f = File::open(&path).unwrap();
        let mut hdr = [0u8; 12];
        use std::io::Read;
        f.read_exact(&mut hdr).unwrap();
        assert_eq!(&hdr[0..4], b"RIFF");
        assert_eq!(&hdr[8..12], b"WAVE");
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 24_000);
        assert_eq!(spec.bits_per_sample, 16);
        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn atomic_overwrite_existing_dest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exists.wav");
        // Pre-create destination (the Windows rename failure case).
        write_wav_i16_mono_atomic(&path, &[1i16, 2, 3], 16_000).unwrap();
        let replacement = vec![9i16, 8, 7, 6];
        write_wav_i16_mono_atomic(&path, &replacement, 24_000).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 24_000);
        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded, replacement);
    }
}
