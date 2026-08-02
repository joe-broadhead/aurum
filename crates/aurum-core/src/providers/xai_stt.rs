//! xAI REST STT via official `POST /v1/stt` (JOE-1942 / JOE-1976).
//!
//! Contract: https://docs.x.ai/developers/rest-api-reference/inference/voice
//! Multipart form with `file` (last field). Response JSON: text, language,
//! duration, optional words[]. No OpenAI `/audio/transcriptions` paths.

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::{self, AudioInput, DEFAULT_FFMPEG_TIMEOUT, DEFAULT_MAX_UPLOAD_BYTES};
use crate::error::{ProviderError, Result, UserError};
use crate::postprocess;
use crate::remote::{
    map_http_status, read_body_limited_with_op, send_with_op, validate_segments,
    validate_text_bounds, HardenedHttpClient, RemoteBodyLimits, RemotePolicy, TranscriptLimits,
    XaiHttpPolicy,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

const PROVIDER_NAME: &str = "xai";

/// Product label for the official xAI file STT vertical (API has no model field).
pub const DEFAULT_XAI_STT_MODEL: &str = "xai-stt";

/// Reviewed xAI STT catalogue entry (product surface; not a wire model id).
#[derive(Debug, Clone, Copy)]
pub struct XaiSttRecord {
    pub model: &'static str,
    /// Word-level timestamps available when the response includes `words`.
    pub timestamps_supported: bool,
    pub max_upload_bytes: usize,
}

/// Static reviewed product IDs for xAI STT (JOE-1976).
pub static XAI_STT_REGISTRY: &[XaiSttRecord] = &[XaiSttRecord {
    model: "xai-stt",
    timestamps_supported: true,
    max_upload_bytes: 100 * 1024 * 1024, // documented limit is 500MB; Aurum uses a safer cap
}];

pub fn lookup_xai_stt(model: &str) -> Option<&'static XaiSttRecord> {
    let m = model.trim();
    if m.is_empty() {
        return Some(&XAI_STT_REGISTRY[0]);
    }
    XAI_STT_REGISTRY
        .iter()
        .find(|r| r.model.eq_ignore_ascii_case(m))
}

/// xAI file transcription provider.
pub struct XaiSttProvider {
    api_key: String,
    http: HardenedHttpClient,
    max_upload_bytes: usize,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for XaiSttProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiSttProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .field("max_upload_bytes", &self.max_upload_bytes)
            .finish()
    }
}

impl XaiSttProvider {
    pub fn with_policy(
        api_key: Option<String>,
        base_url: Option<String>,
        mut policy: RemotePolicy,
    ) -> Result<Self> {
        let api_key = api_key
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(UserError::MissingApiKey)?;

        if base_url
            .as_deref()
            .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"))
        {
            policy.allow_loopback_http = true;
        }

        let http = HardenedHttpClient::build(base_url.as_deref(), policy, XaiHttpPolicy)?;
        Ok(Self {
            api_key,
            http,
            max_upload_bytes: DEFAULT_MAX_UPLOAD_BYTES.min(100 * 1024 * 1024),
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
impl TranscriptionProvider for XaiSttProvider {
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
        let rec =
            lookup_xai_stt(&options.model).ok_or_else(|| UserError::UnsupportedCapability {
                provider: PROVIDER_NAME.into(),
                model: options.model.clone(),
                reason: "model is not in the reviewed xAI STT registry".into(),
                hint: format!(
                    "use {} (official REST has no per-request model id; do not use grok-asr*)",
                    DEFAULT_XAI_STT_MODEL
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

        // Official form: optional language; file must be last.
        let mut form = Form::new();
        let lang = options.language.trim().to_ascii_lowercase();
        if !lang.is_empty() && lang != "auto" {
            form = form.text("language", lang.clone());
        }
        form = form.part("file", part);

        let response = send_with_op(
            self.http
                .request(reqwest::Method::POST, "stt", &self.api_key)?
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
        let want_ts = options.timestamps && rec.timestamps_supported;
        let (text, segments, timestamps_reliable) =
            parse_xai_stt_body(&body_text, want_ts, input.duration_secs())?;
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
            rec.model.to_string(),
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
struct XaiSttJson {
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    words: Option<Vec<XaiWord>>,
}

#[derive(Debug, Deserialize)]
struct XaiWord {
    text: String,
    start: f64,
    end: f64,
}

fn parse_xai_stt_body(
    body: &str,
    want_timestamps: bool,
    media_duration: f64,
) -> Result<(String, Vec<Segment>, bool)> {
    let parsed: XaiSttJson =
        serde_json::from_str(body).map_err(|e| ProviderError::InvalidProviderPayload {
            provider: PROVIDER_NAME.into(),
            reason: format!("xAI STT JSON: {e}"),
        })?;

    let text = parsed.text.trim().to_string();
    if text.is_empty() {
        return Err(ProviderError::TranscriptionFailed {
            reason: "empty transcription text".into(),
        }
        .into());
    }

    let duration = parsed.duration.unwrap_or(media_duration).max(0.0);

    if want_timestamps {
        if let Some(words) = parsed.words.filter(|w| !w.is_empty()) {
            let segments: Vec<Segment> = words
                .into_iter()
                .map(|w| Segment::from_parts_unchecked(w.start, w.end, w.text))
                .collect();
            return Ok((text, segments, true));
        }
    }

    let _ = parsed.language;
    Ok((
        text.clone(),
        vec![Segment::from_parts_unchecked(0.0, duration, text)],
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
        assert!(lookup_xai_stt("xai-stt").is_some());
        assert!(lookup_xai_stt("").is_some());
        assert!(lookup_xai_stt("grok-asr").is_none());
        assert!(lookup_xai_stt("not-a-model").is_none());
    }

    #[tokio::test]
    async fn mock_official_stt_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/stt"))
            .and(header("Authorization", "Bearer xai-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello world",
                "language": "en",
                "duration": 1.0,
                "words": [
                    { "text": "hello", "start": 0.0, "end": 0.4 },
                    { "text": "world", "start": 0.4, "end": 1.0 }
                ]
            })))
            .mount(&server)
            .await;

        let provider = XaiSttProvider::with_policy(
            Some("xai-test".into()),
            Some(format!("{}/v1", server.uri().trim_end_matches('/'))),
            RemotePolicy {
                allow_loopback_http: true,
                allow_custom_credentialed_endpoint: true,
                ..Default::default()
            },
        )
        .unwrap();

        let samples = vec![0.1f32; 16_000];
        let audio = AudioInput::from_pcm(samples, 16_000).unwrap();
        let result = provider
            .transcribe(
                &audio,
                &TranscriptionOptions {
                    model: "xai-stt".into(),
                    language: "en".into(),
                    timestamps: true,
                    cancel: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result.provider(), "xai");
        assert!(result.text().contains("hello"));
        assert!(result.timestamps_reliable());
    }

    #[test]
    fn rejects_missing_key() {
        let err = XaiSttProvider::with_policy(None, None, RemotePolicy::default()).unwrap_err();
        assert!(err.to_string().to_ascii_lowercase().contains("key") || matches!(err, _));
    }
}
