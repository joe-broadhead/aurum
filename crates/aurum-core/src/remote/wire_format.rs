//! Fail-closed remote TTS response format binding (JOE-1977).
//!
//! Expected wire format is fixed by the request/capability. Content-Type must
//! agree; missing or contradictory types never default to PCM.

use super::openai_speech::parse_pcm_content_type;
use crate::audio::EncodedAudioFormat;
use crate::error::{ProviderError, Result};

/// What the client requested / capability promised for a remote TTS body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedWireFormat {
    /// Raw little-endian signed 16-bit PCM mono or multi-channel.
    PcmS16Le { sample_rate_hz: u32, channels: u16 },
    /// MPEG Layer III (normalized via supervised FFmpeg).
    Mp3,
    /// RIFF/WAVE (in-process decode).
    Wav,
}

impl ExpectedWireFormat {
    pub fn pcm(sample_rate_hz: u32, channels: u16) -> Self {
        Self::PcmS16Le {
            sample_rate_hz,
            channels: channels.max(1),
        }
    }
}

fn looks_like_text_envelope(body: &[u8]) -> bool {
    let trimmed = body
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| &body[i..])
        .unwrap_or(&[]);
    if trimmed.is_empty() {
        return false;
    }
    trimmed.starts_with(b"{")
        || trimmed.starts_with(b"[")
        || trimmed.starts_with(b"<")
        || trimmed.windows(5).any(|w| w.eq_ignore_ascii_case(b"<html"))
        || trimmed.windows(5).any(|w| w.eq_ignore_ascii_case(b"<?xml"))
}

/// Resolve encoded format from expected request format + response Content-Type.
///
/// `body` is only inspected for structural contradictions (JSON/HTML/RIFF magic);
/// it is never echoed into errors.
pub fn resolve_encoded_format(
    provider: &str,
    expected: ExpectedWireFormat,
    content_type: &str,
    body: &[u8],
) -> Result<EncodedAudioFormat> {
    let ct_raw = content_type.trim();
    if ct_raw.is_empty() {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "missing Content-Type on remote audio response".into(),
        }
        .into());
    }

    let ct_lower = ct_raw.to_ascii_lowercase();
    let primary = ct_lower.split(';').next().unwrap_or("").trim();

    // Text/JSON envelopes are never valid audio for any expected format.
    if primary.starts_with("application/json")
        || primary.starts_with("text/")
        || primary.starts_with("application/xml")
        || primary.starts_with("application/xhtml")
        || looks_like_text_envelope(body)
    {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "remote audio response is not binary audio (JSON/text/HTML envelope)".into(),
        }
        .into());
    }

    match expected {
        ExpectedWireFormat::PcmS16Le {
            sample_rate_hz,
            channels,
        } => {
            if primary.contains("mpeg") || primary == "audio/mp3" {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "expected PCM but Content-Type is MP3".into(),
                }
                .into());
            }
            if primary.contains("wav") || body.starts_with(b"RIFF") {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "expected PCM but response looks like WAV".into(),
                }
                .into());
            }
            if primary == "application/octet-stream" || primary == "binary/octet-stream" {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "generic Content-Type cannot be accepted as PCM".into(),
                }
                .into());
            }
            // Allowlisted PCM MIME families.
            let pcm_ok = primary == "audio/pcm"
                || primary == "audio/l16"
                || primary == "audio/l16;rate"
                || primary.starts_with("audio/pcm")
                || primary.starts_with("audio/l16");
            // ElevenLabs and some vendors use bare audio/pcm without params.
            if !pcm_ok && primary != "audio/raw" {
                // Some APIs return "audio/s16le" or similar.
                if primary != "audio/s16le" && primary != "audio/x-raw" {
                    return Err(ProviderError::InvalidProviderPayload {
                        provider: provider.into(),
                        reason: format!(
                            "Content-Type '{primary}' is not an allowlisted PCM type for requested PCM"
                        ),
                    }
                    .into());
                }
            }

            // If rate/channels appear in Content-Type, they must match expectation.
            if let Some((rate, ch)) = parse_pcm_content_type(ct_raw) {
                if rate != sample_rate_hz {
                    return Err(ProviderError::InvalidProviderPayload {
                        provider: provider.into(),
                        reason: format!(
                            "PCM rate {rate} does not match requested {sample_rate_hz}"
                        ),
                    }
                    .into());
                }
                if ch != channels {
                    return Err(ProviderError::InvalidProviderPayload {
                        provider: provider.into(),
                        reason: format!("PCM channels {ch} do not match requested {channels}"),
                    }
                    .into());
                }
            }

            if !body.len().is_multiple_of(2) {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "PCM body length is not a multiple of 2".into(),
                }
                .into());
            }
            if body.is_empty() {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "empty PCM body".into(),
                }
                .into());
            }

            Ok(EncodedAudioFormat::PcmS16Le {
                sample_rate_hz,
                channels,
            })
        }
        ExpectedWireFormat::Mp3 => {
            let mp3_ok = primary.contains("mpeg")
                || primary == "audio/mp3"
                || primary == "audio/x-mp3"
                || primary == "audio/mpeg3";
            if !mp3_ok {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: format!(
                        "Content-Type '{primary}' is not allowlisted MP3 for requested MP3"
                    ),
                }
                .into());
            }
            Ok(EncodedAudioFormat::Mp3)
        }
        ExpectedWireFormat::Wav => {
            let wav_ok = primary == "audio/wav"
                || primary == "audio/wave"
                || primary == "audio/x-wav"
                || primary == "audio/vnd.wave";
            if !wav_ok {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: format!(
                        "Content-Type '{primary}' is not allowlisted WAV for requested WAV"
                    ),
                }
                .into());
            }
            if body.len() >= 4 && !body.starts_with(b"RIFF") {
                return Err(ProviderError::InvalidProviderPayload {
                    provider: provider.into(),
                    reason: "WAV Content-Type without RIFF magic".into(),
                }
                .into());
            }
            Ok(EncodedAudioFormat::Wav)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_requires_allowlisted_mime() {
        let exp = ExpectedWireFormat::pcm(24_000, 1);
        let pcm = [0u8; 4];
        assert!(resolve_encoded_format("t", exp, "audio/pcm;rate=24000;channels=1", &pcm).is_ok());
        assert!(resolve_encoded_format("t", exp, "", &pcm).is_err());
        assert!(resolve_encoded_format("t", exp, "application/octet-stream", &pcm).is_err());
        assert!(resolve_encoded_format("t", exp, "application/json", b"{}").is_err());
        assert!(resolve_encoded_format("t", exp, "audio/mpeg", &pcm).is_err());
        assert!(resolve_encoded_format("t", exp, "audio/pcm", b"RIFF....").is_err());
        assert!(resolve_encoded_format("t", exp, "text/html", b"<html>").is_err());
        // Rate mismatch
        assert!(resolve_encoded_format("t", exp, "audio/pcm;rate=16000;channels=1", &pcm).is_err());
    }

    #[test]
    fn json_body_never_pcm() {
        let exp = ExpectedWireFormat::pcm(24_000, 1);
        let err =
            resolve_encoded_format("t", exp, "audio/pcm", b"{\"error\":\"nope\"}").unwrap_err();
        let s = err.to_string();
        assert!(!s.contains("nope")); // no body echo
        assert!(s.contains("envelope") || s.contains("not binary"));
    }

    #[test]
    fn mp3_and_wav_allowlists() {
        assert!(resolve_encoded_format(
            "t",
            ExpectedWireFormat::Mp3,
            "audio/mpeg",
            &[0xFF, 0xFB, 0, 0]
        )
        .is_ok());
        assert!(
            resolve_encoded_format("t", ExpectedWireFormat::Wav, "audio/wav", b"RIFF....WAVE")
                .is_ok()
        );
        assert!(
            resolve_encoded_format("t", ExpectedWireFormat::Wav, "audio/wav", b"not-riff").is_err()
        );
    }
}
