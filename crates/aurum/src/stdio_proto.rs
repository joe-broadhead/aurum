//! JSONL speech sidecar protocol (GitHub #140).
//!
//! Aurum is ears/mouth only. The harness (pi, OpenCode, …) is the brain.
//! Framing matches Pi RPC: one JSON object per LF line, optional trailing CR.
//! Human logs and tracing stay on **stderr**. PCM and secrets never appear.

use serde_json::{json, Value};
use std::io::{self, Write};

pub const PROTO_V: u32 = 1;
/// Fail closed on a single stdin line larger than this (bytes).
pub const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InCmd {
    /// Host-owned recording (absolute path). Aurum does not capture.
    Transcribe {
        path: String,
    },
    /// Write TTS WAV to a host-owned absolute path. Does not play.
    Synthesize {
        text: String,
        path: String,
    },
    /// Play TTS on the default speaker (optional; desktop hosts prefer synthesize).
    Speak {
        text: String,
    },
    EndTurn,
    Abort,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutEvent {
    Ready {
        stt_provider: String,
        tts_provider: String,
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
            Ok(InCmd::Synthesize { text, path })
        }
        "speak" => {
            let text = required_text(&v, "text")?;
            Ok(InCmd::Speak { text })
        }
        "end_turn" => Ok(InCmd::EndTurn),
        "abort" => Ok(InCmd::Abort),
        "shutdown" => Ok(InCmd::Shutdown),
        other => Err(format!("unknown type '{other}'")),
    }
}

pub fn event_json(ev: &OutEvent) -> Value {
    match ev {
        OutEvent::Ready {
            stt_provider,
            tts_provider,
        } => json!({
            "v": PROTO_V,
            "type": "ready",
            "stt_provider": stt_provider,
            "tts_provider": tts_provider,
            "caps": ["transcribe", "synthesize", "speak"],
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
        } => json!({
            "v": PROTO_V,
            "type": "synthesized",
            "path": path,
            "duration_ms": duration_ms,
            "provider": provider,
            "model": model,
            "voice": voice,
            "sample_rate_hz": sample_rate_hz,
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

    #[test]
    fn parse_speak_and_crlf() {
        let c = parse_line("{\"v\":1,\"type\":\"speak\",\"text\":\"Hello.\"}\r").unwrap();
        assert_eq!(
            c,
            InCmd::Speak {
                text: "Hello.".into()
            }
        );
    }

    #[test]
    fn parse_end_abort_shutdown() {
        assert_eq!(
            parse_line("{\"type\":\"end_turn\"}").unwrap(),
            InCmd::EndTurn
        );
        assert_eq!(parse_line("{\"type\":\"abort\"}").unwrap(), InCmd::Abort);
        assert_eq!(
            parse_line("{\"v\":1,\"type\":\"shutdown\"}").unwrap(),
            InCmd::Shutdown
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
        let ready = event_json(&OutEvent::Ready {
            stt_provider: "local".into(),
            tts_provider: "local".into(),
        })
        .to_string();
        assert!(ready.contains("transcribe") && ready.contains("synthesize"));
    }

    #[test]
    fn parse_transcribe_and_synthesize() {
        let t = parse_line("{\"type\":\"transcribe\",\"path\":\"/tmp/a.wav\"}").unwrap();
        assert_eq!(
            t,
            InCmd::Transcribe {
                path: "/tmp/a.wav".into()
            }
        );
        let s = parse_line("{\"type\":\"synthesize\",\"text\":\"Hi.\",\"path\":\"/tmp/out.wav\"}")
            .unwrap();
        assert_eq!(
            s,
            InCmd::Synthesize {
                text: "Hi.".into(),
                path: "/tmp/out.wav".into()
            }
        );
        assert!(parse_line("{\"type\":\"transcribe\",\"path\":\"\"}").is_err());
        assert!(
            parse_line("{\"type\":\"synthesize\",\"text\":\"Hi.\",\"path\":\"x\\ny\"}").is_err()
        );
    }
}
