//! Device I/O for supported `aurum converse --mic` (CLI). Not used by `--stdio` daemon.
//!
//! `aurum-core` stays device-agnostic. Capture is downmixed to mono in the
//! callback; resample to 16 kHz happens on the worker.

use aurum_core::error::{Result, UserError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfig, SupportedStreamConfigRange};
use std::sync::mpsc;
use std::time::Duration;

/// Default-input capture. Yields **device-rate mono f32** chunks.
pub struct MicCapture {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    rx: mpsc::Receiver<Vec<f32>>,
}

impl MicCapture {
    /// Start capture on the system default input.
    pub fn start() -> Result<Self> {
        Self::start_with_device(None)
    }

    /// Start capture on `device_name` (case-insensitive substring) or the default.
    pub fn start_with_device(device_name: Option<&str>) -> Result<Self> {
        let device = select_input_device(device_name)?;
        let supported = device
            .default_input_config()
            .map_err(|e| UserError::Other {
                message: format!("microphone config: {e}"),
            })?;
        let supported = select_supported_config(supported, || {
            device.supported_input_configs().map_err(|e| {
                UserError::Other {
                    message: format!("microphone supported configs: {e}"),
                }
                .into()
            })
        })?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(48);

        let stream = match format {
            SampleFormat::F32 => build_input::<f32>(&device, &config, channels, tx)?,
            SampleFormat::I16 => build_input::<i16>(&device, &config, channels, tx)?,
            SampleFormat::U16 => build_input::<u16>(&device, &config, channels, tx)?,
            other => {
                return Err(UserError::Other {
                    message: format!("unsupported microphone sample format {other}"),
                }
                .into());
            }
        };
        stream.play().map_err(|e| UserError::Other {
            message: format!("microphone start: {e}"),
        })?;
        Ok(Self {
            _stream: stream,
            sample_rate,
            rx,
        })
    }

    /// Input device name reported by the platform, if the device has one.
    pub fn describe(device_name: Option<&str>) -> Result<String> {
        let device = select_input_device(device_name)?;
        Ok(device_display_name(&device))
    }

    /// Names of every available input device.
    pub fn input_device_names() -> Vec<String> {
        device_names(cpal::default_host().input_devices())
    }

    /// Names of every available output device.
    pub fn output_device_names() -> Vec<String> {
        device_names(cpal::default_host().output_devices())
    }

    /// Blocking recv with timeout (None = timeout).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Vec<f32>> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }
}

fn device_names<I>(devices: std::result::Result<I, cpal::Error>) -> Vec<String>
where
    I: Iterator<Item = cpal::Device>,
{
    let Ok(devices) = devices else {
        return Vec::new();
    };
    devices.map(|d| device_display_name(&d)).collect::<Vec<_>>()
}

/// cpal 0.18 reports a structured description instead of `Device::name()`.
fn device_display_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown device".to_string())
}

/// Probe the default input without opening a stream (doctor, sidecar `ready`).
///
/// A stream start can trigger the OS permission prompt; a config query cannot,
/// so this is safe to run in a read-only diagnostic. A denied grant shows up
/// here as "no device" or a config error, which the doctor reports as a warning
/// with the System Settings next step.
pub fn probe_microphone() -> aurum_core::doctor::MicrophoneSurface {
    let devices = MicCapture::input_device_names();
    match select_input_device(None) {
        Ok(device) => match device.default_input_config() {
            Ok(cfg) => aurum_core::doctor::MicrophoneSurface {
                probed: true,
                available: true,
                name: Some(device_display_name(&device)),
                devices,
                reason: Some(format!(
                    "{} Hz, {} channel(s), {}",
                    cfg.sample_rate(),
                    cfg.channels(),
                    cfg.sample_format()
                )),
            },
            Err(e) => aurum_core::doctor::MicrophoneSurface {
                probed: true,
                available: false,
                name: Some(device_display_name(&device)),
                devices,
                reason: Some(format!(
                    "device present but its default input config failed: {e}"
                )),
            },
        },
        Err(e) => aurum_core::doctor::MicrophoneSurface {
            probed: true,
            available: false,
            name: None,
            devices,
            reason: Some(e.to_string()),
        },
    }
}

/// Resolve an input device: the first whose name contains `name`, else the default.
///
/// A name that matches nothing is an error rather than a silent fallback, so a
/// host that asked for a specific microphone does not record from another one.
pub fn select_input_device(name: Option<&str>) -> Result<cpal::Device> {
    select_device(name, true)
}

/// Resolve an output device by name, or the default.
pub fn select_output_device(name: Option<&str>) -> Result<cpal::Device> {
    select_device(name, false)
}

fn select_device(name: Option<&str>, input: bool) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        None => {
            let found = if input {
                host.default_input_device()
            } else {
                host.default_output_device()
            };
            found.ok_or_else(|| {
                UserError::Other {
                    message: if input {
                        "no default microphone found".to_string()
                    } else {
                        "no default speaker found".to_string()
                    },
                }
                .into()
            })
        }
        Some(wanted) => {
            let needle = wanted.to_ascii_lowercase();
            let devices = if input {
                host.input_devices()
            } else {
                host.output_devices()
            }
            .map_err(|e| UserError::Other {
                message: format!("cannot enumerate audio devices: {e}"),
            })?;
            let mut available = Vec::new();
            for device in devices {
                let device_name = device_display_name(&device);
                if device_name.to_ascii_lowercase().contains(&needle) {
                    return Ok(device);
                }
                available.push(device_name);
            }
            Err(UserError::Other {
                message: format!(
                    "no {} device matches '{wanted}' (available: {})",
                    if input { "input" } else { "output" },
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
            }
            .into())
        }
    }
}

fn build_input<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: u16,
    tx: mpsc::SyncSender<Vec<f32>>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Send + 'static,
    f32: FromSampleLoose<T>,
{
    let ch = channels.max(1) as usize;
    let stream = device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let n = data.len() / ch;
                if n == 0 {
                    return;
                }
                let mut mono = Vec::with_capacity(n);
                for frame in 0..n {
                    let mut acc = 0.0f32;
                    for c in 0..ch {
                        acc += f32::from_sample_loose(data[frame * ch + c]);
                    }
                    mono.push(acc / ch as f32);
                }
                let _ = tx.try_send(mono);
            },
            |e| eprintln!("aurum: microphone error: {e}"),
            None,
        )
        .map_err(|e| UserError::Other {
            message: format!("microphone stream: {e}"),
        })?;
    Ok(stream)
}

/// Play mono i16 PCM on the default output (blocking until done).
pub fn play_i16_mono(pcm: &[i16], sample_rate_hz: u32) -> Result<()> {
    play_i16_mono_on(pcm, sample_rate_hz, None)
}

/// Play mono i16 PCM on `device_name` (substring) or the default output.
pub fn play_i16_mono_on(pcm: &[i16], sample_rate_hz: u32, device_name: Option<&str>) -> Result<()> {
    if pcm.is_empty() {
        return Ok(());
    }
    let device = select_output_device(device_name)?;
    let supported = device
        .default_output_config()
        .map_err(|e| UserError::Other {
            message: format!("speaker config: {e}"),
        })?;
    let supported = select_supported_config(supported, || {
        device.supported_output_configs().map_err(|e| {
            UserError::Other {
                message: format!("speaker supported configs: {e}"),
            }
            .into()
        })
    })?;
    let out_rate = supported.sample_rate();
    let channels = supported.channels();
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let mut f32_mono: Vec<f32> = pcm.iter().map(|s| f32::from(*s) / 32768.0).collect();
    if out_rate != sample_rate_hz {
        f32_mono = resample_mono(&f32_mono, sample_rate_hz, out_rate);
    }

    let (tx_done, rx_done) = mpsc::channel::<()>();
    let shared = std::sync::Arc::new(std::sync::Mutex::new(PlayBuf {
        samples: f32_mono,
        pos: 0,
        channels: channels.max(1) as usize,
        done: false,
    }));
    let buf = std::sync::Arc::clone(&shared);
    let tx_cb = tx_done;

    let stream = match format {
        SampleFormat::F32 => build_output::<f32>(&device, &config, buf, tx_cb)?,
        SampleFormat::I16 => build_output::<i16>(&device, &config, buf, tx_cb)?,
        SampleFormat::U16 => build_output::<u16>(&device, &config, buf, tx_cb)?,
        other => {
            return Err(UserError::Other {
                message: format!("unsupported speaker sample format {other}"),
            }
            .into());
        }
    };
    stream.play().map_err(|e| UserError::Other {
        message: format!("speaker start: {e}"),
    })?;
    let _ = rx_done.recv_timeout(Duration::from_secs(120));
    // `done` fires when the last sample has been copied into a callback buffer,
    // not when the device has played it. Dropping the stream immediately
    // truncates the tail and shows up as a device underrun, so let the queued
    // buffer drain first.
    std::thread::sleep(Duration::from_millis(200));
    // Keep stream alive until the drain completes.
    drop(stream);
    Ok(())
}

struct PlayBuf {
    samples: Vec<f32>,
    pos: usize,
    channels: usize,
    done: bool,
}

fn build_output<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    buf: std::sync::Arc<std::sync::Mutex<PlayBuf>>,
    done: mpsc::Sender<()>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Send + 'static,
    T: FromF32,
{
    let stream = device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                let mut g = buf.lock().unwrap_or_else(|e| e.into_inner());
                if g.done {
                    for s in data.iter_mut() {
                        *s = T::from_f32(0.0);
                    }
                    return;
                }
                let ch = g.channels.max(1);
                let frames = data.len() / ch;
                for i in 0..frames {
                    let v = if g.pos < g.samples.len() {
                        let s = g.samples[g.pos];
                        g.pos += 1;
                        s
                    } else {
                        0.0
                    };
                    let sample = T::from_f32(v);
                    for c in 0..ch {
                        data[i * ch + c] = sample;
                    }
                }
                if g.pos >= g.samples.len() && !g.done {
                    g.done = true;
                    let _ = done.send(());
                }
            },
            |e| tracing::debug!("speaker stream error: {e}"),
            None,
        )
        .map_err(|e| UserError::Other {
            message: format!("speaker stream: {e}"),
        })?;
    Ok(stream)
}

/// CPAL 0.18 can prefer I32/I24 over I16. Keep a supported default unchanged;
/// otherwise select a device-advertised format our callbacks can convert.
fn select_supported_config<I>(
    preferred: SupportedStreamConfig,
    alternatives: impl FnOnce() -> Result<I>,
) -> Result<SupportedStreamConfig>
where
    I: Iterator<Item = SupportedStreamConfigRange>,
{
    let supported = |format| {
        matches!(
            format,
            SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
        )
    };
    if supported(preferred.sample_format()) {
        return Ok(preferred);
    }
    let range = alternatives()?
        .filter(|config| supported(config.sample_format()))
        .max_by(SupportedStreamConfigRange::cmp_default_heuristics)
        .ok_or_else(|| UserError::Other {
            message: format!(
                "device default sample format {} is unsupported and no f32, i16, or u16 configuration is available",
                preferred.sample_format()
            ),
        })?;
    // Preserve the preferred rate when supported; never request an unadvertised
    // rate. The existing resampler handles a different capture/playback rate.
    let rate = preferred
        .sample_rate()
        .clamp(range.min_sample_rate(), range.max_sample_rate());
    Ok(range.with_sample_rate(rate))
}

/// Linear resample, mono.
pub fn resample_mono(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == 0 || to_hz == 0 || input.is_empty() {
        return Vec::new();
    }
    if from_hz == to_hz {
        return input.to_vec();
    }
    let n_out = ((input.len() as u64) * u64::from(to_hz) / u64::from(from_hz)).max(1) as usize;
    let mut out = Vec::with_capacity(n_out);
    let ratio = f64::from(from_hz) / f64::from(to_hz);
    for i in 0..n_out {
        let src = i as f64 * ratio;
        let j = src.floor() as usize;
        let frac = (src - j as f64) as f32;
        let a = input.get(j).copied().unwrap_or(0.0);
        let b = input.get(j.saturating_add(1)).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

trait FromSampleLoose<T> {
    fn from_sample_loose(s: T) -> f32;
}

impl FromSampleLoose<f32> for f32 {
    fn from_sample_loose(s: f32) -> f32 {
        s
    }
}
impl FromSampleLoose<i16> for f32 {
    fn from_sample_loose(s: i16) -> f32 {
        f32::from(s) / 32768.0
    }
}
impl FromSampleLoose<u16> for f32 {
    fn from_sample_loose(s: u16) -> f32 {
        (f32::from(s) / 32768.0) - 1.0
    }
}

trait FromF32 {
    fn from_f32(v: f32) -> Self;
}
impl FromF32 for f32 {
    fn from_f32(v: f32) -> Self {
        v.clamp(-1.0, 1.0)
    }
}
impl FromF32 for i16 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}
impl FromF32 for u16 {
    fn from_f32(v: f32) -> Self {
        ((v.clamp(-1.0, 1.0) + 1.0) * 32767.0) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_range(
        format: SampleFormat,
        min_rate: u32,
        max_rate: u32,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            2,
            min_rate,
            max_rate,
            cpal::SupportedBufferSize::Unknown,
            format,
        )
    }

    #[test]
    fn supported_device_default_does_not_enumerate_alternatives() {
        let preferred = config_range(SampleFormat::F32, 48_000, 48_000).with_sample_rate(48_000);
        let selected = select_supported_config(
            preferred,
            || -> Result<std::iter::Empty<SupportedStreamConfigRange>> {
                panic!("a supported default must not require enumeration")
            },
        )
        .unwrap();
        assert_eq!(selected.sample_format(), SampleFormat::F32);
        assert_eq!(selected.sample_rate(), 48_000);
        assert_eq!(selected.channels(), 2);
    }

    #[test]
    fn high_precision_device_defaults_fall_back_to_supported_pcm() {
        for format in [SampleFormat::I32, SampleFormat::I24, SampleFormat::F64] {
            let preferred = config_range(format, 48_000, 48_000).with_sample_rate(48_000);
            let selected = select_supported_config(preferred, || {
                Ok([
                    config_range(format, 44_100, 96_000),
                    config_range(SampleFormat::I16, 44_100, 96_000),
                ]
                .into_iter())
            })
            .unwrap();
            assert_eq!(selected.sample_format(), SampleFormat::I16);
            assert_eq!(selected.sample_rate(), 48_000);
            assert_eq!(selected.channels(), 2);
        }
    }

    #[test]
    fn fallback_rate_stays_inside_the_selected_device_range() {
        for preferred_rate in [8_000, 96_000] {
            let preferred = config_range(SampleFormat::I32, preferred_rate, preferred_rate)
                .with_sample_rate(preferred_rate);
            let selected = select_supported_config(preferred, || {
                Ok([config_range(SampleFormat::I16, 44_100, 48_000)].into_iter())
            })
            .unwrap();
            assert_eq!(selected.sample_rate(), preferred_rate.clamp(44_100, 48_000));
        }
    }

    #[test]
    fn device_without_a_supported_sample_format_returns_an_error() {
        let preferred = config_range(SampleFormat::I24, 48_000, 48_000).with_sample_rate(48_000);
        let err = select_supported_config(preferred, || {
            Ok([config_range(SampleFormat::I24, 48_000, 48_000)].into_iter())
        })
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("no f32, i16, or u16 configuration"));
    }

    #[test]
    fn resample_identity() {
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_mono(&x, 16_000, 16_000), x);
    }

    #[test]
    fn resample_down() {
        let x = vec![0.0, 1.0, 0.0, -1.0];
        let y = resample_mono(&x, 16_000, 8_000);
        assert_eq!(y.len(), 2);
    }
}
