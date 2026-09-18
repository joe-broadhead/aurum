//! Device I/O for supported `aurum converse --mic` (CLI). Not used by `--stdio` daemon.
//!
//! `aurum-core` stays device-agnostic. Capture is downmixed to mono in the
//! callback; resample to 16 kHz happens on the worker.

use aurum_core::error::{Result, UserError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::sync::mpsc;
use std::time::Duration;

/// Default-input capture. Yields **device-rate mono f32** chunks.
pub struct MicCapture {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    rx: mpsc::Receiver<Vec<f32>>,
}

impl MicCapture {
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| UserError::Other {
                message: "no default microphone found".into(),
            })?;
        let supported = device
            .default_input_config()
            .map_err(|e| UserError::Other {
                message: format!("microphone config: {e}"),
            })?;
        let sample_rate = supported.sample_rate().0;
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

    /// Blocking recv with timeout (None = timeout).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Vec<f32>> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
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
            config,
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
    if pcm.is_empty() {
        return Ok(());
    }
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| UserError::Other {
            message: "no default speaker found".into(),
        })?;
    let supported = device
        .default_output_config()
        .map_err(|e| UserError::Other {
            message: format!("speaker config: {e}"),
        })?;
    let out_rate = supported.sample_rate().0;
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
    // Keep stream alive until recv returns.
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
            config,
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
            |e| eprintln!("aurum: speaker error: {e}"),
            None,
        )
        .map_err(|e| UserError::Other {
            message: format!("speaker stream: {e}"),
        })?;
    Ok(stream)
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
