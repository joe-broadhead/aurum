//! JSONL speech sidecar protocol (GitHub #140).
//!
//! Aurum is ears/mouth only. The harness (pi, OpenCode, …) is the brain.
//! Framing matches Pi RPC: one JSON object per LF line, optional trailing CR.
//! Human logs and tracing stay on **stderr**. PCM and secrets never appear.
//!
//! Capability negotiation lives in `ready.caps` (see
//! [`aurum_core::surface::sidecar_caps`]), so a host reads one event instead of
//! probing for invented capability names or grepping `--help`.

use serde_json::{json, Value};
use std::io::{self, Write};

pub const PROTO_V: u32 = 1;
/// Fail closed on a single stdin line larger than this (bytes).
pub const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum InCmd {
    /// Host-owned recording (absolute path). Aurum does not capture.
    Transcribe {
        path: String,
    },
    /// Write TTS WAV to a host-owned absolute path. Does not play.
    Synthesize {
        text: String,
        path: String,
        /// Optional speaking-rate multiplier (0.5..=2.0).
        speaking_rate: Option<f32>,
    },
    /// Play TTS on the default speaker (optional; path-based hosts prefer synthesize).
    Speak {
        text: String,
        /// Optional speaking-rate multiplier (0.5..=2.0).
        speaking_rate: Option<f32>,
    },
    /// Download/load the STT model and run one warm decode, then re-emit `ready`.
    Preload,
    EndTurn,
    Abort,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutEvent {
    Ready {
        stt_provider: String,
        tts_provider: String,
        /// Capability ids from [`aurum_core::surface::sidecar_caps`].
        caps: Vec<String>,
        /// True when the model had already been warmed before `ready`.
        preloaded: bool,
        version: String,
    },
    /// A preload is running; the host can show progress instead of a spinner.
    Warming {
        model: String,
    },
    /// The preload finished and the next decode is warm.
    Preloaded {
        model: String,
        load_ms: u64,
        warm_ms: u64,
        total_ms: u64,
    },
    UserFinal {
        text: String,
        provider: String,
        model: String,
    },
    Listening,
    Synthesized {
        path: String,
        duration_ms: u64,
        provider: String,
        model: String,
        voice: String,
        sample_rate_hz: u32,
        speaking_rate: f32,
    },
    Error {
        message: String,
        /// Coarse group: user | environment | provider | internal.
        category: String,
    },
    Shutdown,
}

impl OutEvent {
    pub fn error(category: &str, message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            category: category.to_string(),
        }
    }
}

/// Lowest and highest accepted speaking-rate multipliers.
pub const SPEAKING_RATE_RANGE: (f32, f32) = (0.5, 2.0);

pub fn parse_line(line: &str) -> Result<InCmd, String> {
    let line = line.strip_suffix('\r').unwrap_or(line).trim();
    if line.is_empty() {
        return Err("empty line".into());
    }
    let v: Value = serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;
    let ver = v
        .get("v")
        .and_then(|x| x.as_u64())
        .unwrap_or(u64::from(PROTO_V));
    if ver != u64::from(PROTO_V) {
        return Err(format!("unsupported protocol v={ver} (expected {PROTO_V})"));
    }
    let typ = v
        .get("type")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "missing type".to_string())?;
    match typ {
        "transcribe" => {
            let path = required_path(&v, "path")?;
            Ok(InCmd::Transcribe { path })
        }
        "synthesize" => {
            let text = required_text(&v, "text")?;
            let path = required_path(&v, "path")?;
            let speaking_rate = optional_rate(&v)?;
            Ok(InCmd::Synthesize {
                text,
                path,
                speaking_rate,
            })
        }
        "speak" => {
            let text = required_text(&v, "text")?;
            let speaking_rate = optional_rate(&v)?;
            Ok(InCmd::Speak {
                text,
                speaking_rate,
            })
        }
        "preload" => Ok(InCmd::Preload),
        "end_turn" => Ok(InCmd::EndTurn),
        "abort" => Ok(InCmd::Abort),
        "shutdown" => Ok(InCmd::Shutdown),
        other => Err(format!("unknown type '{other}'")),
    }
}

fn optional_rate(v: &Value) -> Result<Option<f32>, String> {
    let Some(raw) = v.get("speaking_rate").or_else(|| v.get("rate")) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let rate = raw
        .as_f64()
        .ok_or_else(|| "speaking_rate must be a number".to_string())? as f32;
    if !rate.is_finite() || !(SPEAKING_RATE_RANGE.0..=SPEAKING_RATE_RANGE.1).contains(&rate) {
        return Err(format!(
            "speaking_rate must be between {} and {} (got {rate})",
            SPEAKING_RATE_RANGE.0, SPEAKING_RATE_RANGE.1
        ));
    }
    Ok(Some(rate))
}

pub fn event_json(ev: &OutEvent) -> Value {
    match ev {
        OutEvent::Ready {
            stt_provider,
            tts_provider,
            caps,
            preloaded,
            version,
        } => json!({
            "v": PROTO_V,
            "type": "ready",
            "stt_provider": stt_provider,
            "tts_provider": tts_provider,
            "caps": caps,
            "preloaded": preloaded,
            "version": version,
        }),
        OutEvent::Warming { model } => json!({
            "v": PROTO_V,
            "type": "warming",
            "model": model,
        }),
        OutEvent::Preloaded {
            model,
            load_ms,
            warm_ms,
            total_ms,
        } => json!({
            "v": PROTO_V,
            "type": "preloaded",
            "model": model,
            "load_ms": load_ms,
            "warm_ms": warm_ms,
            "total_ms": total_ms,
        }),
        OutEvent::UserFinal {
            text,
            provider,
            model,
        } => json!({
            "v": PROTO_V,
            "type": "user_final",
            "text": text,
            "provider": provider,
            "model": model,
        }),
        OutEvent::Listening => json!({ "v": PROTO_V, "type": "listening" }),
        OutEvent::Synthesized {
            path,
            duration_ms,
            provider,
            model,
            voice,
            sample_rate_hz,
            speaking_rate,
        } => json!({
            "v": PROTO_V,
            "type": "synthesized",
            "path": path,
            "duration_ms": duration_ms,
            "provider": provider,
            "model": model,
            "voice": voice,
            "sample_rate_hz": sample_rate_hz,
            "speaking_rate": speaking_rate,
        }),
        OutEvent::Error { message, category } => json!({
            "v": PROTO_V,
            "type": "error",
            "category": category,
            "message": message,
        }),
        OutEvent::Shutdown => json!({ "v": PROTO_V, "type": "shutdown" }),
    }
}

fn required_text(v: &Value, key: &str) -> Result<String, String> {
    let text = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(format!("{key} is empty"));
    }
    Ok(text)
}

fn required_path(v: &Value, key: &str) -> Result<String, String> {
    let path = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if path.is_empty() {
        return Err(format!("{key} is empty"));
    }
    if path.contains('\n') || path.contains('\0') {
        return Err(format!("{key} contains invalid characters"));
    }
    Ok(path)
}

/// Write one event and flush. Callers must use stdout only for this protocol.
pub fn write_event(ev: &OutEvent) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{}", event_json(ev))?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> OutEvent {
        OutEvent::Ready {
            stt_provider: "local".into(),
            tts_provider: "local".into(),
            caps: aurum_core::sidecar_caps(false)
                .into_iter()
                .map(str::to_string)
                .collect(),
            preloaded: false,
            version: "test".into(),
        }
    }

    #[test]
    fn parse_speak_and_crlf() {
        let c = parse_line("{\"v\":1,\"type\":\"speak\",\"text\":\"Hello.\"}\r").unwrap();
        assert_eq!(
            c,
            InCmd::Speak {
                text: "Hello.".into(),
                speaking_rate: None
            }
        );
    }

    #[test]
    fn parse_end_abort_shutdown_preload() {
        assert_eq!(
            parse_line("{\"type\":\"end_turn\"}").unwrap(),
            InCmd::EndTurn
        );
        assert_eq!(parse_line("{\"type\":\"abort\"}").unwrap(), InCmd::Abort);
        assert_eq!(
            parse_line("{\"v\":1,\"type\":\"shutdown\"}").unwrap(),
            InCmd::Shutdown
        );
        assert_eq!(
            parse_line("{\"type\":\"preload\"}").unwrap(),
            InCmd::Preload
        );
    }

    #[test]
    fn reject_empty_speak_and_unknown() {
        assert!(parse_line("{\"type\":\"speak\",\"text\":\"  \"}").is_err());
        assert!(parse_line("{\"type\":\"prompt\",\"message\":\"x\"}").is_err());
        assert!(parse_line("not-json").is_err());
    }

    #[test]
    fn reject_v2() {
        assert!(parse_line("{\"v\":2,\"type\":\"end_turn\"}").is_err());
    }

    #[test]
    fn speaking_rate_is_optional_and_bounded() {
        let s = parse_line("{\"type\":\"synthesize\",\"text\":\"Hi.\",\"path\":\"/tmp/o.wav\",\"speaking_rate\":1.25}")
            .unwrap();
        assert_eq!(
            s,
            InCmd::Synthesize {
                text: "Hi.".into(),
                path: "/tmp/o.wav".into(),
                speaking_rate: Some(1.25)
            }
        );
        assert!(parse_line(
            "{\"type\":\"synthesize\",\"text\":\"Hi.\",\"path\":\"/tmp/o.wav\",\"speaking_rate\":9}"
        )
        .is_err());
        assert!(
            parse_line("{\"type\":\"speak\",\"text\":\"Hi.\",\"speaking_rate\":\"fast\"}").is_err()
        );
    }

    #[test]
    fn events_are_single_line() {
        let s = event_json(&OutEvent::UserFinal {
            text: "hi".into(),
            provider: "local".into(),
            model: "tiny-q5_1".into(),
        })
        .to_string();
        assert!(!s.contains('\n'));
        assert!(s.contains("user_final"));
        assert!(!s.to_ascii_lowercase().contains("pcm"));

        let ready = event_json(&ready()).to_string();
        assert!(ready.contains("transcribe") && ready.contains("synthesize"));
        for cap in ["synthesize_speaking_rate", "synthesize_abort", "long_audio"] {
            assert!(ready.contains(cap), "ready.caps missing {cap}");
        }
    }
}
