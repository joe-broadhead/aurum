//! `aurum record` and `aurum play` — device capture and playback on the same
//! `cpal` stack `aurum converse --mic` already uses (A2 / A3).
//!
//! Hosts (opencode-voice, Jelly) previously composed `ffmpeg -f avfoundation`
//! and `afplay` to get audio in and out. That put a second audio stack, a
//! subprocess and a device-name parser in every host. These two commands keep
//! capture and playback inside Aurum: one device enumeration, one resampler,
//! one WAV writer, no ffmpeg and no `HOME` rewriting.
//!
//! `record` writes **16 kHz mono PCM** WAV, the format the STT path and the
//! sidecar already accept, so a host can hand the file straight to
//! `transcribe`. `play` reads a PCM WAV at its native rate.

use crate::audio_io::{play_i16_mono_on, MicCapture};
use aurum_core::audio::WHISPER_SAMPLE_RATE;
use aurum_core::error::{Result, UserError};
use clap::Parser;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `aurum record` — capture a WAV for STT or a host.
#[derive(Debug, Parser)]
pub struct RecordCli {
    /// Write the recording here (16 kHz mono PCM WAV).
    #[arg(short = 'O', long = "output-file", value_name = "PATH")]
    pub output_file: PathBuf,

    /// Input device: case-insensitive substring of its name. Default: system default.
    #[arg(long, value_name = "NAME")]
    pub device: Option<String>,

    /// Stop after this many seconds (default: until Ctrl-C or SIGTERM).
    #[arg(long, value_name = "SECS")]
    pub seconds: Option<f64>,

    /// Emit JSONL input-level events on stdout (`{"type":"level",...}`, ~10/s).
    #[arg(long = "level-events")]
    pub level_events: bool,

    /// Overwrite an existing non-empty output file.
    #[arg(long)]
    pub force: bool,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// `aurum play` — play a WAV on the default output (or a named one).
#[derive(Debug, Parser)]
pub struct PlayCli {
    /// WAV file to play (PCM, any rate; downmixed to mono).
    #[arg(value_name = "WAV_FILE")]
    pub wav_file: PathBuf,

    /// Playback speed multiplier, 0.5..=2.0 (changes rate and pitch, like tape).
    #[arg(long, value_name = "RATE")]
    pub rate: Option<f32>,

    /// Output device: case-insensitive substring of its name. Default: system default.
    #[arg(long, value_name = "NAME")]
    pub device: Option<String>,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// One level event: about ten per second while recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelFrame {
    pub rms: f32,
    pub peak: f32,
}

/// Incremental linear resampler that carries phase across callback chunks.
///
/// Resampling each callback independently would drift and click at every
/// boundary; this keeps the fractional cursor and the last sample so a long
/// recording is one continuous signal.
#[derive(Debug, Clone)]
pub struct StreamResampler {
    ratio: f64,
    cursor: f64,
    carry: Option<f32>,
}

impl StreamResampler {
    pub fn new(from_hz: u32, to_hz: u32) -> Self {
        assert!(from_hz > 0 && to_hz > 0, "sample rates must be non-zero");
        Self {
            ratio: f64::from(from_hz) / f64::from(to_hz),
            cursor: 0.0,
            carry: None,
        }
    }

    /// Resample the next chunk; the returned samples continue the previous ones.
    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let n = input.len();
        // Same rate: pass everything through. Falling through the interpolator
        // would defer one sample per chunk, and a 16 kHz device is common.
        if (self.ratio - 1.0).abs() < f64::EPSILON {
            self.carry = Some(input[n - 1]);
            return input.to_vec();
        }
        let sample = |i: isize| -> f32 {
            if i < 0 {
                self.carry.unwrap_or(input[0])
            } else if (i as usize) < n {
                input[i as usize]
            } else {
                input[n - 1]
            }
        };
        let mut out = Vec::with_capacity(((n as f64) / self.ratio).ceil() as usize + 2);
        // Only interpolate while both taps are inside this chunk; the tail is
        // finished by the next chunk (or the flush at end of capture).
        let limit = n as f64 - 1.0;
        while self.cursor < limit {
            let i = self.cursor.floor() as isize;
            let frac = (self.cursor - i as f64) as f32;
            let a = sample(i);
            let b = sample(i + 1);
            out.push(a + (b - a) * frac);
            self.cursor += self.ratio;
        }
        self.carry = Some(input[n - 1]);
        self.cursor -= n as f64;
        out
    }

    /// Emit the final sample so the tail is not dropped.
    pub fn finish(&mut self) -> Option<f32> {
        self.carry
    }
}

/// Convert device-rate mono f32 into 16 kHz i16, carrying phase across chunks.
fn to_whisper_pcm(resampler: &mut StreamResampler, chunk: &[f32], out: &mut Vec<i16>) {
    for v in resampler.push(chunk) {
        out.push((v.clamp(-1.0, 1.0) * 32_767.0) as i16);
    }
}

fn level_of(chunk: &[f32]) -> LevelFrame {
    if chunk.is_empty() {
        return LevelFrame {
            rms: 0.0,
            peak: 0.0,
        };
    }
    let mut sum = 0.0f64;
    let mut peak = 0.0f32;
    for v in chunk {
        sum += f64::from(*v) * f64::from(*v);
        peak = peak.max(v.abs());
    }
    LevelFrame {
        rms: (sum / chunk.len() as f64).sqrt() as f32,
        peak,
    }
}

fn ensure_writable(path: &Path, force: bool) -> Result<()> {
    if !force {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > 0 {
                return Err(UserError::Other {
                    message: format!(
                        "{} already exists ({} bytes); pass --force to overwrite",
                        path.display(),
                        meta.len()
                    ),
                }
                .into());
            }
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| UserError::Other {
                message: format!("create {}: {e}", parent.display()),
            })?;
        }
    }
    Ok(())
}

/// Write 16-bit mono PCM atomically (temp file + rename beside the target).
fn write_wav(path: &Path, samples: &[i16], rate_hz: u32) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut tmp = tempfile::NamedTempFile::new_in(parent.unwrap_or_else(|| Path::new(".")))
        .map_err(|e| UserError::Other {
            message: format!("create temp WAV beside {}: {e}", path.display()),
        })?;
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate_hz,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer =
            hound::WavWriter::new(tmp.as_file_mut(), spec).map_err(|e| UserError::Other {
                message: format!("write WAV header: {e}"),
            })?;
        for s in samples {
            writer.write_sample(*s).map_err(|e| UserError::Other {
                message: format!("write WAV samples: {e}"),
            })?;
        }
        writer.finalize().map_err(|e| UserError::Other {
            message: format!("finalize WAV: {e}"),
        })?;
    }
    tmp.persist(path).map_err(|e| UserError::Other {
        message: format!("move WAV into {}: {}", path.display(), e.error),
    })?;
    Ok(())
}

/// `aurum record`: capture until `--seconds`, Ctrl-C or SIGTERM.
pub async fn run_record(cli: RecordCli) -> Result<()> {
    if let Some(secs) = cli.seconds {
        if !secs.is_finite() || secs <= 0.0 {
            return Err(UserError::Other {
                message: "--seconds must be a positive number".into(),
            }
            .into());
        }
    }
    ensure_writable(&cli.output_file, cli.force)?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.store(true, Ordering::SeqCst);
        });
    }
    tokio::task::spawn_blocking(move || record_blocking(cli, stop))
        .await
        .map_err(|e| UserError::Other {
            message: format!("record worker: {e}"),
        })?
}

fn record_blocking(cli: RecordCli, stop: Arc<AtomicBool>) -> Result<()> {
    let capture = MicCapture::start_with_device(cli.device.as_deref())?;
    let device_rate = capture.sample_rate;
    let mut resampler = StreamResampler::new(device_rate, WHISPER_SAMPLE_RATE);
    let mut pcm: Vec<i16> = Vec::new();
    let started = Instant::now();
    let deadline = cli.seconds.map(Duration::from_secs_f64);
    let mut last_level = Instant::now();
    let mut peak_peak = 0.0f32;
    let stdout = std::io::stdout();

    if cli.verbose {
        eprintln!(
            "aurum: recording from {} at {} Hz → {} Hz mono",
            cli.device.as_deref().unwrap_or("default input"),
            device_rate,
            WHISPER_SAMPLE_RATE
        );
    }

    while !stop.load(Ordering::SeqCst) {
        if let Some(deadline) = deadline {
            if started.elapsed() >= deadline {
                break;
            }
        }
        let Some(chunk) = capture.recv_timeout(Duration::from_millis(80)) else {
            continue;
        };
        if chunk.is_empty() {
            continue;
        }
        peak_peak = peak_peak.max(level_of(&chunk).peak);
        to_whisper_pcm(&mut resampler, &chunk, &mut pcm);
        if cli.level_events && last_level.elapsed() >= Duration::from_millis(100) {
            last_level = Instant::now();
            let level = level_of(&chunk);
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "{}",
                json!({
                    "type": "level",
                    "t_ms": started.elapsed().as_millis(),
                    "rms": round4(level.rms),
                    "peak": round4(level.peak),
                })
            );
            let _ = out.flush();
        }
    }

    if let Some(tail) = resampler.finish() {
        pcm.push((tail.clamp(-1.0, 1.0) * 32_767.0) as i16);
    }
    drop(capture);
    write_wav(&cli.output_file, &pcm, WHISPER_SAMPLE_RATE)?;

    let seconds = pcm.len() as f64 / f64::from(WHISPER_SAMPLE_RATE);
    if cli.level_events {
        let mut out = stdout.lock();
        let _ = writeln!(
            out,
            "{}",
            json!({
                "type": "recorded",
                "path": cli.output_file.display().to_string(),
                "seconds": round3(seconds),
                "samples": pcm.len(),
                "sample_rate_hz": WHISPER_SAMPLE_RATE,
                "peak": round4(peak_peak),
            })
        );
        let _ = out.flush();
    } else if cli.verbose {
        eprintln!(
            "aurum: recorded {:.2}s → {}",
            seconds,
            cli.output_file.display()
        );
    }
    Ok(())
}

fn round4(v: f32) -> f64 {
    (f64::from(v) * 10_000.0).round() / 10_000.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// `aurum play`: decode a PCM WAV and play it, optionally faster or slower.
pub async fn run_play(cli: PlayCli) -> Result<()> {
    let rate = cli.rate.unwrap_or(1.0);
    if !rate.is_finite() || !(0.5..=2.0).contains(&rate) {
        return Err(UserError::Other {
            message: format!("--rate must be between 0.5 and 2.0 (got {rate})"),
        }
        .into());
    }
    let wav = aurum_core::read_wav_native(&cli.wav_file)?;
    if wav.samples.is_empty() {
        return Err(UserError::InvalidAudio {
            reason: format!("{} has no audio samples", cli.wav_file.display()),
        }
        .into());
    }
    let playback_rate = if (rate - 1.0).abs() < f32::EPSILON {
        wav.sample_rate_hz
    } else {
        ((wav.sample_rate_hz as f32) * rate).round() as u32
    };
    if cli.verbose {
        eprintln!(
            "aurum: playing {} ({:.2}s, {} Hz{} )",
            cli.wav_file.display(),
            wav.duration_secs(),
            wav.sample_rate_hz,
            if playback_rate == wav.sample_rate_hz {
                String::new()
            } else {
                format!(", rate {rate}x → {playback_rate} Hz")
            }
        );
    }
    tokio::task::spawn_blocking(move || {
        play_i16_mono_on(&wav.samples, playback_rate, cli.device.as_deref())
    })
    .await
    .map_err(|e| UserError::Other {
        message: format!("play worker: {e}"),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_is_continuous_across_chunks() {
        // 48 kHz → 16 kHz is exactly 3:1, so chunk boundaries must not add or
        // drop samples.
        let input: Vec<f32> = (0..4_800).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut whole = StreamResampler::new(48_000, 16_000);
        let mut a = whole.push(&input);

        let mut chunked = StreamResampler::new(48_000, 16_000);
        let mut b = Vec::new();
        for chunk in input.chunks(441) {
            b.extend(chunked.push(chunk));
        }
        a.extend(whole.finish());
        b.extend(chunked.finish());
        assert!(
            (a.len() as i64 - b.len() as i64).abs() <= 2,
            "length drift: {} vs {}",
            a.len(),
            b.len()
        );
        for (x, y) in a.iter().zip(b.iter()) {
            assert!((x - y).abs() < 1e-4, "{x} != {y}");
        }
    }

    #[test]
    fn resampler_identity_keeps_every_sample() {
        let mut r = StreamResampler::new(16_000, 16_000);
        let input = [0.0f32, 0.5, -0.5, 1.0];
        assert_eq!(r.push(&input), input);
    }

    #[test]
    fn level_of_silence_is_zero() {
        let level = level_of(&[0.0; 320]);
        assert_eq!(level.rms, 0.0);
        assert_eq!(level.peak, 0.0);
    }

    #[test]
    fn level_of_full_scale_is_one() {
        let level = level_of(&[1.0, -1.0]);
        assert!((level.rms - 1.0).abs() < 1e-6);
        assert!((level.peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn write_wav_round_trips_through_the_native_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        let samples: Vec<i16> = vec![0, 100, -100, 32_000];
        write_wav(&path, &samples, WHISPER_SAMPLE_RATE).unwrap();
        let read = aurum_core::read_wav_native(&path).unwrap();
        assert_eq!(read.samples, samples);
        assert_eq!(read.sample_rate_hz, WHISPER_SAMPLE_RATE);
        assert_eq!(read.channels, 1);
        assert!((read.duration_secs() - 4.0 / 16_000.0).abs() < 1e-9);
    }

    #[test]
    fn ensure_writable_refuses_existing_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exists.wav");
        std::fs::write(&path, b"data").unwrap();
        assert!(ensure_writable(&path, false).is_err());
        assert!(ensure_writable(&path, true).is_ok());
    }
}
