//! Lightweight reproducible benchmark helpers (JOE-1606).
//!
//! Full hardware matrix runs live in `scripts/bench_smoke.sh` and produce
//! machine-readable JSON. This module defines the schema and micro-measurements
//! that unit tests / PR smoke can exercise without pinned model downloads.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Named reference profile for budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchProfile {
    /// Apple Silicon + Metal whisper.
    AppleSiliconMetal,
    /// Generic x86_64 Linux CPU.
    LinuxX86Cpu,
    /// Constrained mobile/low-memory.
    Constrained,
}

impl BenchProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppleSiliconMetal => "apple_silicon_metal",
            Self::LinuxX86Cpu => "linux_x86_cpu",
            Self::Constrained => "constrained",
        }
    }

    pub fn detect() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return Self::AppleSiliconMetal;
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            return Self::LinuxX86Cpu;
        }
        #[allow(unreachable_code)]
        Self::Constrained
    }
}

/// One timed sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchSample {
    pub name: String,
    pub iterations: u32,
    pub wall_ms_p50: f64,
    pub wall_ms_p95: f64,
    pub wall_ms_mean: f64,
    /// Optional peak RSS estimate in bytes (0 if not measured).
    pub peak_rss_bytes: u64,
    pub notes: String,
}

/// Machine-readable benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub schema_version: u32,
    pub profile: BenchProfile,
    pub commit: String,
    pub target: String,
    pub features: String,
    pub warm: bool,
    pub samples: Vec<BenchSample>,
}

impl BenchReport {
    pub fn new(profile: BenchProfile, warm: bool) -> Self {
        Self {
            schema_version: 1,
            profile,
            commit: std::env::var("GITHUB_SHA")
                .or_else(|_| std::env::var("AURUM_BENCH_COMMIT"))
                .unwrap_or_else(|_| "unknown".into()),
            target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            features: std::env::var("AURUM_BENCH_FEATURES").unwrap_or_else(|_| "default".into()),
            warm,
            samples: Vec::new(),
        }
    }
}

/// Run `f` for `iters` times; return sorted wall durations.
pub fn time_iters<F: FnMut()>(mut f: F, iters: u32) -> Vec<Duration> {
    let mut times = Vec::with_capacity(iters as usize);
    for _ in 0..iters {
        let t0 = Instant::now();
        f();
        times.push(t0.elapsed());
    }
    times.sort();
    times
}

pub fn percentile_ms(sorted: &[Duration], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx =
        ((p.clamp(0.0, 1.0) * (sorted.len() as f64 - 1.0)).round() as usize).min(sorted.len() - 1);
    sorted[idx].as_secs_f64() * 1000.0
}

pub fn mean_ms(sorted: &[Duration]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let sum: f64 = sorted.iter().map(|d| d.as_secs_f64() * 1000.0).sum();
    sum / sorted.len() as f64
}

/// Micro-bench: ring-buffer push throughput (PR smoke safe).
pub fn sample_pcm_push(iters: u32) -> BenchSample {
    use crate::pcm::PcmBuffer;
    let chunk = vec![0.01f32; 1600]; // 100 ms
    let times = time_iters(
        || {
            let mut buf = PcmBuffer::dictation();
            for _ in 0..20 {
                let _ = buf.push(&chunk);
            }
            let _ = buf.rms();
        },
        iters,
    );
    BenchSample {
        name: "pcm_ring_push_20x100ms".into(),
        iterations: iters,
        wall_ms_p50: percentile_ms(&times, 0.50),
        wall_ms_p95: percentile_ms(&times, 0.95),
        wall_ms_mean: mean_ms(&times),
        peak_rss_bytes: 0,
        notes: "no model download; CPU only".into(),
    }
}

/// Micro-bench: SRT stream format for N segments.
pub fn sample_srt_stream(segments: usize, iters: u32) -> BenchSample {
    use crate::output::{write_result_with_limit, OutputFormat};
    use crate::providers::{Segment, TranscriptionResult};
    let segs: Vec<Segment> = (0..segments)
        .map(|i| {
            Segment::from_parts_unchecked(
                i as f64 * 0.5,
                i as f64 * 0.5 + 0.4,
                format!("segment number {i}"),
            )
        })
        .collect();
    let result = TranscriptionResult::local(
        "bench".into(),
        segs,
        Some("en".into()),
        "bench".into(),
        segments as f64,
    );
    let times = time_iters(
        || {
            let mut out = Vec::with_capacity(segments * 64);
            let _ = write_result_with_limit(&result, OutputFormat::Srt, &mut out, 16 * 1024 * 1024);
        },
        iters,
    );
    BenchSample {
        name: format!("srt_stream_{segments}_segments"),
        iterations: iters,
        wall_ms_p50: percentile_ms(&times, 0.50),
        wall_ms_p95: percentile_ms(&times, 0.95),
        wall_ms_mean: mean_ms(&times),
        peak_rss_bytes: 0,
        notes: "streaming SRT writer".into(),
    }
}

/// Soft regression thresholds for smoke samples (profile-specific multipliers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokeBudgets {
    pub pcm_push_p95_ms: f64,
    pub srt_1k_p95_ms: f64,
}

impl SmokeBudgets {
    pub fn for_profile(p: BenchProfile) -> Self {
        match p {
            BenchProfile::Constrained => Self {
                pcm_push_p95_ms: 50.0,
                srt_1k_p95_ms: 200.0,
            },
            _ => Self {
                pcm_push_p95_ms: 25.0,
                srt_1k_p95_ms: 100.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_micro_benches_run() {
        let profile = BenchProfile::detect();
        let mut report = BenchReport::new(profile, true);
        report.samples.push(sample_pcm_push(5));
        report.samples.push(sample_srt_stream(200, 3));
        let budgets = SmokeBudgets::for_profile(profile);
        let pcm = report
            .samples
            .iter()
            .find(|s| s.name.contains("pcm"))
            .unwrap();
        assert!(
            pcm.wall_ms_p95 < budgets.pcm_push_p95_ms * 20.0,
            "pcm p95 too high: {}",
            pcm.wall_ms_p95
        );
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("schema_version"));
    }
}
