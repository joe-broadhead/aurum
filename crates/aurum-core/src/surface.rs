//! The machine-readable statement of what this build can do (#10).
//!
//! Hosts should read this instead of parsing prose from `--help`, `models` or
//! `doctor`. One place owns the protocol version, the sidecar capability ids
//! and the platform identity so `ready.caps`, `aurum capabilities --json` and
//! the docs cannot drift apart.
//!
//! This module is deliberately static: no model load, no network, no device
//! access. A host can call it cheaply during install or warmup.

use serde::{Deserialize, Serialize};

/// Schema version for the `aurum capabilities --json` document.
pub const SURFACE_SCHEMA_VERSION: u32 = 1;

/// `converse --stdio` protocol version (`v` in every sidecar line).
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest audio a sidecar `transcribe` accepts, in seconds (2 hours).
///
/// The live `--mic` path still ends a turn at [`crate::live::LiveSessionConfig`]'s
/// `max_utterance_secs`; a host-owned file has no such reason to be short.
pub const SIDECAR_MAX_AUDIO_SECS: f64 = 2.0 * 60.0 * 60.0;

/// Capability ids published in `ready.caps`.
///
/// `speak` is only advertised when `--mic` is on, because without a device the
/// sidecar rejects it and a host should not have to probe.
pub fn sidecar_caps(mic: bool) -> Vec<&'static str> {
    let mut caps = vec![
        "transcribe",
        "synthesize",
        "synthesize_speaking_rate",
        "synthesize_abort",
        "long_audio",
    ];
    if mic {
        caps.push("speak");
    }
    caps
}

/// Platform identity, as a host records it in a support bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub target: String,
}

impl PlatformInfo {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_caps_always_explain_long_audio_and_rate() {
        for mic in [false, true] {
            let caps = sidecar_caps(mic);
            for required in [
                "transcribe",
                "synthesize",
                "synthesize_speaking_rate",
                "synthesize_abort",
                "long_audio",
            ] {
                assert!(caps.contains(&required), "missing {required} (mic={mic})");
            }
            assert_eq!(caps.contains(&"speak"), mic);
        }
    }

    #[test]
    fn platform_names_are_stable_and_non_empty() {
        let p = PlatformInfo::current();
        assert!(!p.os.is_empty() && !p.arch.is_empty() && !p.target.is_empty());
    }
}
