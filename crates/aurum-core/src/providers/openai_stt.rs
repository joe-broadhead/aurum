//! First-party OpenAI STT via dedicated multipart `/audio/transcriptions` (JOE-1940).
//!
//! Uses [`OpenAiHttpPolicy`] only — never OpenRouter keys/headers. Unknown models
//! fail closed against the reviewed catalogue.

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::{self, AudioInput, DEFAULT_FFMPEG_TIMEOUT, DEFAULT_MAX_UPLOAD_BYTES};
use crate::error::{ProviderError, Result, UserError};
use crate::postprocess;
use crate::remote::{
    effective_chunk_secs, map_http_status, read_body_limited_with_op, send_with_op,
    transcribe_maybe_chunked, validate_segments, validate_text_bounds, HardenedHttpClient,
    OpenAiHttpPolicy, RemoteBodyLimits, RemotePolicy, TranscriptLimits,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

const PROVIDER_NAME: &str = "openai";

/// Default reviewed OpenAI transcription model.
pub const DEFAULT_OPENAI_STT_MODEL: &str = "whisper-1";

/// Reviewed OpenAI STT catalogue entry.
#[derive(Debug, Clone, Copy)]
pub struct OpenAiSttRecord {
    pub model: &'static str,
    /// Whether `verbose_json` + segment timestamps are supported.
    pub timestamps_supported: bool,
    pub max_upload_bytes: usize,
}

/// Static reviewed OpenAI STT models (JOE-1940).
pub static OPENAI_STT_REGISTRY: &[OpenAiSttRecord] = &[
    OpenAiSttRecord {
        model: "whisper-1",
        timestamps_supported: true,
        max_upload_bytes: 25 * 1024 * 1024,
    },
    OpenAiSttRecord {
        model: "gpt-4o-mini-transcribe",
        timestamps_supported: false, // JSON text only
        max_upload_bytes: 25 * 1024 * 1024,
    },
    OpenAiSttRecord {
        model: "gpt-4o-transcribe",
        timestamps_supported: false,
        max_upload_bytes: 25 * 1024 * 1024,
    },
];

pub fn lookup_openai_stt(model: &str) -> Option<&'static OpenAiSttRecord> {
    let m = model.trim();
    OPENAI_STT_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// First-party OpenAI transcription provider.
pub struct OpenAiSttProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    max_upload_bytes: usize,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for OpenAiSttProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiSttProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .field("max_upload_bytes", &self.max_upload_bytes)
            .finish()
    }
}

impl OpenAiSttProvider {
    pub fn with_policy(
        api_key: Option<SecretString>,
        base_url: Option<String>,
        mut policy: RemotePolicy,
    ) -> Result<Self> {
        let api_key = api_key
            .filter(|s| !s.expose().trim().is_empty())
            .ok_or(UserError::MissingApiKey)?;

        if base_url
            .as_deref()
            .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"))
        {
            policy.allow_loopback_http = true;
        }

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, OpenAiHttpPolicy)?;
        Ok(Self {
            api_key,
            http,
            max_upload_bytes: DEFAULT_MAX_UPLOAD_BYTES.min(25 * 1024 * 1024),
            governor: ResourceGovernor::process_global(),
        })
    }

    /// Bind an engine-local governor (preferred for long-lived hosts).
    pub fn with_governor(mut self, governor: Arc<ResourceGovernor>) -> Self {
        self.governor = governor;
        self
    }
}

#[async_trait]
impl TranscriptionProvider for OpenAiSttProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Asr
    }

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        // Fail closed on unknown models before any chunk work (JOE-2212).
        let _ =
            lookup_openai_stt(&options.model).ok_or_else(|| UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: options.model.clone(),
                reason: "model is not in the reviewed OpenAI STT registry".into(),
                hint: format!(
                    "use one of: {}",
                    OPENAI_STT_REGISTRY
                        .iter()
                        .map(|r| r.model)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })?;
        transcribe_maybe_chunked(
            input,
            options,
            PROVIDER_NAME,
            effective_chunk_secs(),
            |chunk, opts| async move { self.transcribe_one_shot(&chunk, &opts).await },
        )
        .await
    }
}

impl OpenAiSttProvider {
    /// Single remote request for one audio window (used by chunk-and-stitch).
    async fn transcribe_one_shot(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        let rec =
            lookup_openai_stt(&options.model).ok_or_else(|| UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: options.model.clone(),
                reason: "model is not in the reviewed OpenAI STT registry".into(),
                hint: format!(
                    "use one of: {}",
                    OPENAI_STT_REGISTRY
                        .iter()
                        .map(|r| r.model)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })?;

        let op = crate::runtime::OpContext::from_optional_cancel(options.cancel.clone());
        op.check()?;
        op.emit("stt", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
        op.check()?;
        op.emit("stt", "encode");

        let upload_cap = self.max_upload_bytes.min(rec.max_upload_bytes);
        let (upload_path, format) = audio::encode_for_upload_with_timeout(
            input.samples().as_ref(),
            upload_cap,
            DEFAULT_FFMPEG_TIMEOUT,
            Some(op.cancel.clone()),
        )
        .await?;
        op.check()?;
        let cleanup = PathGuard(upload_path.clone());

        let meta = tokio::fs::metadata(&upload_path)
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("stat upload artifact: {e}"),
            })?;
        let encoded_len = meta.len() as usize;
        if encoded_len > upload_cap {
            return Err(UserError::AudioTooLarge {
                decoded_bytes: encoded_len,
                max_bytes: upload_cap,
            }
            .into());
        }

        let filename = format!("audio.{format}");
        let mime = match format {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            _ => "application/octet-stream",
        };
        op.emit("stt", "upload");
        op.check()?;
        let part = Part::file(&upload_path)
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("multipart file part: {e}"),
            })?
            .file_name(filename)
            .mime_str(mime)
            .map_err(|e| ProviderError::Other {
                message: format!("multipart mime: {e}"),
            })?;

        let mut form = Form::new()
            .text("model", options.model.clone())
            .part("file", part);
        let lang = options.language.trim().to_ascii_lowercase();
        if !lang.is_empty() && lang != "auto" {
            form = form.text("language", lang.clone());
        }
        // Only request verbose_json when the model supports timestamps.
        let want_ts = options.timestamps && rec.timestamps_supported;
        if want_ts {
            form = form.text("response_format", "verbose_json");
        } else {
            form = form.text("response_format", "json");
        }

        let response = send_with_op(
            self.http
                .request(
                    reqwest::Method::POST,
                    "audio/transcriptions",
                    self.api_key.expose(),
                )?
                .multipart(form),
            &op,
            PROVIDER_NAME,
        )
        .await?;
        drop(cleanup);
        op.check()?;
        op.emit("stt", "read_body");

        let status = response.status();
        let body = read_body_limited_with_op(response, PROVIDER_NAME, RemoteBodyLimits::stt(), &op)
            .await?;
        let body_text = String::from_utf8_lossy(&body).into_owned();
        map_http_status(PROVIDER_NAME, status, &body_text)?;

        op.emit("stt", "parse");
        let (text, segments, timestamps_reliable) =
            parse_transcriptions_body(&body_text, want_ts, input.duration_secs())?;
        validate_text_bounds(&text, None, TranscriptLimits::default(), PROVIDER_NAME)?;
        validate_segments(
            &segments,
            input.duration_secs(),
            TranscriptLimits::default(),
            PROVIDER_NAME,
        )?;

        let mut result = TranscriptionResult::openrouter(
            text,
            segments,
            if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            },
            options.model.clone(),
            input.duration_secs(),
            want_ts,
        );
        result.set_provider(PROVIDER_NAME);
        result.set_backend_kind(BackendKind::Asr);
        result.set_timestamps_reliable(timestamps_reliable);
        op.emit("stt", "done");
        Ok(postprocess::normalize_result(result))
    }
}

struct PathGuard(PathBuf);
impl Drop for PathGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionsJson {
    text: String,
    #[serde(default)]
    segments: Option<Vec<TranscriptionsSegment>>,
}

#[derive(Debug, Deserialize)]
struct TranscriptionsSegment {
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
}

fn parse_transcriptions_body(
    body: &str,
    want_timestamps: bool,
    duration: f64,
) -> Result<(String, Vec<Segment>, bool)> {
    use crate::remote::TimestampSource;

    if !body.trim_start().starts_with('{') {
        let text = body.trim().to_string();
        if text.is_empty() {
            return Err(ProviderError::TranscriptionFailed {
                reason: "empty transcription response".into(),
            }
            .into());
        }
        return Ok((
            text.clone(),
            vec![Segment::from_parts_with_source(
                0.0,
                duration,
                text,
                TimestampSource::SyntheticSpan,
            )],
            false,
        ));
    }

    let parsed: TranscriptionsJson =
        serde_json::from_str(body).map_err(|e| ProviderError::InvalidProviderPayload {
            provider: PROVIDER_NAME.into(),
            reason: format!("transcriptions JSON: {e}"),
        })?;

    let text = parsed.text.trim().to_string();
    if text.is_empty() {
        return Err(ProviderError::TranscriptionFailed {
            reason: "empty transcription text".into(),
        }
        .into());
    }

    if want_timestamps {
        if let Some(raw_segs) = parsed.segments {
            let segments: Vec<Segment> = raw_segs
                .into_iter()
                .map(|s| {
                    Segment::from_parts_with_source(
                        s.start,
                        s.end,
                        s.text,
                        TimestampSource::ProviderSegment,
                    )
                })
                .collect();
            return Ok((text, segments, true));
        }
    }
    Ok((
        text.clone(),
        vec![Segment::from_parts_with_source(
            0.0,
            duration,
            text,
            TimestampSource::SyntheticSpan,
        )],
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn registry_lookup() {
        assert!(lookup_openai_stt("whisper-1").is_some());
        assert!(lookup_openai_stt("not-a-model").is_none());
    }

    #[test]
    fn parse_verbose_json_assigns_provider_segment() {
        let body = r#"{"text":"hello world","segments":[{"start":0.0,"end":0.5,"text":"hello"},{"start":0.5,"end":1.0,"text":"world"}]}"#;
        let (text, segs, reliable) = parse_transcriptions_body(body, true, 1.0).unwrap();
        assert_eq!(text, "hello world");
        assert!(reliable);
        assert_eq!(segs.len(), 2);
        assert!(segs
            .iter()
            .all(|s| s.timestamp_source() == crate::remote::TimestampSource::ProviderSegment));
        assert!(!segs.iter().any(|s| s.timestamp_source().is_approximate()));
    }

    #[test]
    fn parse_plain_text_assigns_synthetic_span() {
        let (text, segs, reliable) = parse_transcriptions_body("hello only", false, 2.0).unwrap();
        assert_eq!(text, "hello only");
        assert!(!reliable);
        assert_eq!(segs.len(), 1);
        assert_eq!(
            segs[0].timestamp_source(),
            crate::remote::TimestampSource::SyntheticSpan
        );
        assert!(segs[0].timestamp_source().is_approximate());
    }

    #[tokio::test]
    async fn mock_json_transcription() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello world"
            })))
            .mount(&server)
            .await;

        let provider = OpenAiSttProvider::with_policy(
            Some("sk-test".into()),
            Some(server.uri()),
            RemotePolicy {
                allow_loopback_http: true,
                allow_custom_credentialed_endpoint: true,
                ..Default::default()
            },
        )
        .unwrap();

        // Use empty-ish PCM that encode may convert; for unit test we need real samples.
        let samples = vec![0.1f32; 16_000]; // 1s @ 16kHz
        let audio = AudioInput::from_pcm(samples, 16_000).unwrap();
        let result = provider
            .transcribe(
                &audio,
                &TranscriptionOptions {
                    model: "whisper-1".into(),
                    language: "en".into(),
                    timestamps: false,
                    cancel: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result.text(), "hello world");
        assert_eq!(result.provider(), "openai");
        assert_eq!(result.backend_kind(), BackendKind::Asr);
    }

    #[tokio::test]
    async fn missing_key_fails() {
        let err = OpenAiSttProvider::with_policy(None, None, RemotePolicy::default()).unwrap_err();
        assert!(
            err.to_string().to_ascii_lowercase().contains("key") || err.to_string().contains("API")
        );
    }
}
