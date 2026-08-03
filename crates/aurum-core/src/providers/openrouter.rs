//! OpenRouter remote transcription provider.
//!
//! Supports two request paths (JOE-1586 / JOE-1829):
//! - **Dedicated ASR** — multipart `POST /audio/transcriptions` for models that
//!   expose a real transcription endpoint (e.g. OpenAI Whisper-class models).
//! - **LLM-assisted** — multimodal `POST /chat/completions` with `input_audio`
//!   (Gemini etc.). Timestamps are unreliable.
//!
//! Path selection: explicit config/CLI mode, or `auto` against the reviewed
//! capability registry (unknown models fail closed — no name guessing).

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::{self, AudioInput, DEFAULT_FFMPEG_TIMEOUT, DEFAULT_MAX_UPLOAD_BYTES};
use crate::error::{ProviderError, Result, UserError};
use crate::postprocess;
use crate::remote::{
    effective_chunk_secs, map_http_status, read_body_limited_with_op, send_with_op,
    transcribe_maybe_chunked, validate_segments, validate_text_bounds, HardenedHttpClient,
    RemoteBodyLimits, RemotePolicy, TranscriptLimits,
};
use crate::runtime::{PermitKind, ResourceGovernor};
use crate::secret::SecretString;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const PROVIDER_NAME: &str = "openrouter";

/// How to route OpenRouter STT requests (JOE-1586 / JOE-1829).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenRouterSttMode {
    /// Capability-registry routing only; unknown models fail closed.
    #[default]
    Auto,
    /// Always multimodal chat completions (`LlmAssisted`).
    Chat,
    /// Always dedicated `/audio/transcriptions` (`Asr`).
    Transcriptions,
}

impl OpenRouterSttMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(Self::Auto),
            "chat" | "llm" | "completions" => Ok(Self::Chat),
            "transcriptions" | "asr" | "dedicated" | "audio" => Ok(Self::Transcriptions),
            other => Err(UserError::Other {
                message: format!(
                    "unknown openrouter STT mode '{other}'\n  \
                     Hint: use one of: auto, chat, transcriptions"
                ),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Chat => "chat",
            Self::Transcriptions => "transcriptions",
        }
    }
}

/// OpenRouter provider with dual STT paths.
///
/// Holds an engine-local [`ResourceGovernor`] when built via the registry/engine.
/// Convenience constructors use [`ResourceGovernor::process_global`] only when no
/// governor is supplied (documented process-global path, not engine isolation).
pub struct OpenRouterProvider {
    api_key: SecretString,
    http: HardenedHttpClient,
    max_upload_bytes: usize,
    stt_mode: OpenRouterSttMode,
    governor: Arc<ResourceGovernor>,
}

impl std::fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("base_url", &self.http.base_url())
            .field("api_key", &"***")
            .field("max_upload_bytes", &self.max_upload_bytes)
            .field("stt_mode", &self.stt_mode)
            .finish()
    }
}

impl OpenRouterProvider {
    pub fn new(api_key: Option<String>, base_url: Option<String>) -> Result<Self> {
        Self::with_policy(
            api_key.map(SecretString::from),
            base_url,
            RemotePolicy::default(),
            OpenRouterSttMode::Auto,
        )
    }

    pub fn with_policy(
        api_key: Option<SecretString>,
        base_url: Option<String>,
        mut policy: RemotePolicy,
        stt_mode: OpenRouterSttMode,
    ) -> Result<Self> {
        let api_key = api_key
            .filter(|s| !s.expose().trim().is_empty())
            .ok_or(UserError::MissingApiKey)?;

        // Wiremock / local tests use loopback HTTP.
        if base_url
            .as_deref()
            .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"))
        {
            policy.allow_loopback_http = true;
        }

        let http = HardenedHttpClient::openrouter(base_url.as_deref(), policy)?;

        Ok(Self {
            api_key,
            http,
            max_upload_bytes: DEFAULT_MAX_UPLOAD_BYTES,
            stt_mode,
            // Process-global only when not attached via factory/engine (JOE-1975).
            governor: ResourceGovernor::process_global(),
        })
    }

    /// Bind an engine-local governor (preferred for long-lived hosts).
    pub fn with_governor(mut self, governor: Arc<ResourceGovernor>) -> Self {
        self.governor = governor;
        self
    }

    pub fn with_stt_mode(mut self, mode: OpenRouterSttMode) -> Self {
        self.stt_mode = mode;
        self
    }

    /// Resolve which path to use for `model` (capability-authoritative, JOE-1829).
    ///
    /// Returns [`UserError::UnsupportedCapability`] when `auto` has no registry entry.
    pub fn resolve_path(&self, model: &str) -> Result<SttPath> {
        use crate::capabilities::{resolve_openrouter_stt_path, OpenRouterSttPath};
        match resolve_openrouter_stt_path(self.stt_mode, model)? {
            OpenRouterSttPath::Chat => Ok(SttPath::Chat),
            OpenRouterSttPath::Transcriptions => Ok(SttPath::Transcriptions),
        }
    }
}

/// Selected request path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttPath {
    Chat,
    Transcriptions,
}

/// Whether the model is a **reviewed** dedicated-ASR registry entry (JOE-1829).
///
/// Prefer [`crate::capabilities::lookup_openrouter_stt`] for full records.
/// Explicit `transcriptions` mode may still target unregistered models.
pub fn looks_like_dedicated_asr(model: &str) -> bool {
    use crate::capabilities::{lookup_openrouter_stt, OpenRouterSttPath};
    lookup_openrouter_stt(model)
        .is_some_and(|r| matches!(r.path, OpenRouterSttPath::Transcriptions))
}

#[async_trait]
impl TranscriptionProvider for OpenRouterProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn backend_kind(&self) -> BackendKind {
        // Default label; actual result uses path-specific backend_kind.
        BackendKind::LlmAssisted
    }

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        // Resolve route once so unknown models fail closed before chunking (JOE-2212).
        let _ = self.resolve_path(&options.model)?;
        transcribe_maybe_chunked(
            input,
            options,
            "openrouter",
            effective_chunk_secs(),
            |chunk, opts| async move { self.transcribe_one_shot(&chunk, &opts).await },
        )
        .await
    }
}

impl OpenRouterProvider {
    async fn transcribe_one_shot(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        let op = options.resolve_op_context();
        op.check()?;
        op.emit("stt", "admit");
        let _permit = self.governor.acquire(PermitKind::Remote, Some(&op))?;
        op.check()?;
        op.emit("stt", "route");
        let path = self.resolve_path(&options.model)?;
        op.emit(
            "stt",
            match path {
                SttPath::Transcriptions => "path=transcriptions",
                SttPath::Chat => "path=chat",
            },
        );
        match path {
            SttPath::Transcriptions => self.transcribe_dedicated(input, options, &op).await,
            SttPath::Chat => self.transcribe_chat(input, options, &op).await,
        }
    }
}

impl OpenRouterProvider {
    async fn transcribe_dedicated(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
        op: &crate::runtime::OpContext,
    ) -> Result<TranscriptionResult> {
        let pcm_bytes = input
            .samples()
            .len()
            .saturating_mul(std::mem::size_of::<f32>());
        op.emit("stt", "encode");
        op.check()?;
        // Propagate cancel + absolute encode deadline (JOE-1648 third-pass).
        let (upload_path, format) = audio::encode_for_upload_with_timeout(
            input.samples().as_ref(),
            self.max_upload_bytes,
            DEFAULT_FFMPEG_TIMEOUT,
            Some(op.cancel.clone()),
        )
        .await?;
        op.check()?;
        let cleanup = scopeguard_path(upload_path.clone());

        let meta = tokio::fs::metadata(&upload_path)
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("stat upload artifact: {e}"),
            })?;
        let encoded_len = meta.len() as usize;
        if encoded_len > self.max_upload_bytes {
            return Err(UserError::AudioTooLarge {
                decoded_bytes: encoded_len,
                max_bytes: self.max_upload_bytes,
            }
            .into());
        }

        tracing::debug!(
            pcm_bytes,
            encoded_bytes = encoded_len,
            format,
            "openrouter dedicated upload artifact ready"
        );

        // Stream multipart file from disk — no full base64, no second full
        // in-memory buffer for the dedicated path (JOE-1603).
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
        if options.timestamps {
            form = form.text("response_format", "verbose_json");
        } else {
            form = form.text("response_format", "json");
        }

        tracing::debug!(
            model = %options.model,
            path = "audio/transcriptions",
            "openrouter dedicated STT request"
        );

        let response = send_with_op(
            self.http
                .request(
                    reqwest::Method::POST,
                    "audio/transcriptions",
                    self.api_key.expose(),
                )?
                .multipart(form),
            op,
            PROVIDER_NAME,
        )
        .await?;

        // Body no longer needs the on-disk artifact.
        drop(cleanup);
        op.check()?;
        op.emit("stt", "read_body");

        let status = response.status();
        let body =
            read_body_limited_with_op(response, PROVIDER_NAME, RemoteBodyLimits::stt(), op).await?;
        let body_text = String::from_utf8_lossy(&body).into_owned();
        map_http_status(PROVIDER_NAME, status, &body_text)?;

        op.emit("stt", "parse");
        let (text, segments, timestamps_reliable) =
            parse_transcriptions_body(&body_text, options.timestamps, input.duration_secs())?;
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
            options.timestamps,
        );
        // Dedicated ASR path.
        result.set_backend_kind(BackendKind::Asr);
        result.set_timestamps_reliable(timestamps_reliable);
        result.set_provider(PROVIDER_NAME.to_string());
        op.emit("stt", "done");
        Ok(postprocess::normalize_result(result))
    }

    async fn transcribe_chat(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
        op: &crate::runtime::OpContext,
    ) -> Result<TranscriptionResult> {
        op.emit("stt", "encode");
        op.check()?;
        let (upload_path, format) = audio::encode_for_upload_with_timeout(
            input.samples().as_ref(),
            self.max_upload_bytes,
            DEFAULT_FFMPEG_TIMEOUT,
            Some(op.cancel.clone()),
        )
        .await?;
        op.check()?;
        let cleanup = scopeguard_path(upload_path.clone());

        let meta = tokio::fs::metadata(&upload_path)
            .await
            .map_err(|e| ProviderError::Other {
                message: format!("stat upload artifact: {e}"),
            })?;
        let encoded_len = meta.len() as usize;
        if encoded_len > self.max_upload_bytes {
            return Err(UserError::AudioTooLarge {
                decoded_bytes: encoded_len,
                max_bytes: self.max_upload_bytes,
            }
            .into());
        }

        // Cap wire bytes after base64 expansion (~4/3) before encoding.
        let b64_est = encoded_len.saturating_mul(4).div_ceil(3);
        if b64_est > self.max_upload_bytes.saturating_mul(2) {
            return Err(UserError::AudioTooLarge {
                decoded_bytes: b64_est,
                max_bytes: self.max_upload_bytes.saturating_mul(2),
            }
            .into());
        }

        // Stream file → base64 without holding the full raw buffer and the
        // encoded string simultaneously (JOE-1603 / JOE-1832).
        op.emit("stt", "base64");
        op.check()?;
        let b64 = {
            use base64::engine::general_purpose::STANDARD;
            use base64::write::EncoderStringWriter;
            use std::io::{Read, Write};
            let mut file = std::fs::File::open(&upload_path).map_err(|e| ProviderError::Other {
                message: format!("open upload for base64: {e}"),
            })?;
            let mut encoder = EncoderStringWriter::new(&STANDARD);
            let mut buf = [0u8; 64 * 1024];
            loop {
                if op.cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled.into());
                }
                let n = file.read(&mut buf).map_err(|e| ProviderError::Other {
                    message: format!("read upload for base64: {e}"),
                })?;
                if n == 0 {
                    break;
                }
                encoder
                    .write_all(&buf[..n])
                    .map_err(|e| ProviderError::Other {
                        message: format!("base64 encode: {e}"),
                    })?;
            }
            encoder.into_inner()
        };
        drop(cleanup);

        let mut prompt =
            String::from("Transcribe the audio verbatim. Reply with ONLY the transcript text");
        if options.timestamps {
            prompt.push_str(
                ", as a JSON object with keys \"text\" (string) and \"segments\" \
                 (array of {\"start\": number, \"end\": number, \"text\": string}) \
                 where times are in seconds. Do not wrap in markdown. \
                 If you cannot produce reliable timestamps, return text only as plain string.",
            );
        } else {
            prompt.push_str(". Do not add commentary, labels, or markdown.");
        }

        let lang = options.language.trim().to_ascii_lowercase();
        if !lang.is_empty() && lang != "auto" {
            prompt.push_str(&format!(" The audio language is \"{lang}\"."));
        }

        op.emit("stt", "upload");
        op.check()?;
        let body = json!({
            "model": options.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": b64,
                            "format": format
                        }
                    }
                ]
            }],
            "temperature": 0,
            "top_p": 1,
        });
        // `b64` is moved into `body`; body is the sole large intermediate.

        tracing::debug!(
            model = %options.model,
            path = "chat/completions",
            "openrouter LLM-assisted STT request"
        );

        let response = send_with_op(
            self.http
                .request(
                    reqwest::Method::POST,
                    "chat/completions",
                    self.api_key.expose(),
                )?
                .header("Content-Type", "application/json")
                .json(&body),
            op,
            PROVIDER_NAME,
        )
        .await?;
        drop(body);
        op.check()?;
        op.emit("stt", "read_body");

        let status = response.status();
        let body_bytes =
            read_body_limited_with_op(response, PROVIDER_NAME, RemoteBodyLimits::chat(), op)
                .await?;
        let body_text = String::from_utf8_lossy(&body_bytes).into_owned();
        map_http_status(PROVIDER_NAME, status, &body_text)?;

        op.emit("stt", "parse");
        let parsed: ChatCompletionResponse = serde_json::from_str(&body_text).map_err(|e| {
            ProviderError::InvalidProviderPayload {
                provider: PROVIDER_NAME.into(),
                reason: format!("invalid JSON: {e}"),
            }
        })?;

        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("")
            .trim()
            .to_string();

        if content.is_empty() {
            return Err(ProviderError::TranscriptionFailed {
                reason: "OpenRouter returned an empty transcript".into(),
            }
            .into());
        }

        let (text, segments) =
            parse_chat_content(&content, options.timestamps, input.duration_secs());
        validate_text_bounds(&text, None, TranscriptLimits::default(), PROVIDER_NAME)?;
        // LLM segments are not trusted for ordering hard-fail; soft-validate only when present.
        if options.timestamps {
            let _ = validate_segments(
                &segments,
                input.duration_secs(),
                TranscriptLimits::default(),
                PROVIDER_NAME,
            );
        }

        let result = TranscriptionResult::openrouter(
            text,
            segments,
            if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            },
            options.model.clone(),
            input.duration_secs(),
            options.timestamps,
        );
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
fn scopeguard_path(path: PathBuf) -> PathGuard {
    PathGuard(path)
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TimestampPayload {
    text: String,
    #[serde(default)]
    segments: Vec<Segment>,
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

    // Plain text response_format=text
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
            // Dedicated verbose_json segments carry provider segment timing.
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

fn parse_chat_content(
    content: &str,
    want_timestamps: bool,
    duration: f64,
) -> (String, Vec<Segment>) {
    use crate::remote::TimestampSource;

    if want_timestamps {
        let cleaned = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        if let Ok(payload) = serde_json::from_str::<TimestampPayload>(cleaned) {
            // LLM-invented timings are never provider-native; force Unavailable.
            let segments: Vec<Segment> = payload
                .segments
                .into_iter()
                .map(|s| {
                    Segment::from_parts_with_source(
                        s.start(),
                        s.end(),
                        s.text().to_string(),
                        TimestampSource::Unavailable,
                    )
                })
                .collect();
            return (payload.text, segments);
        }
    }

    let text = content.to_string();
    let segments = vec![Segment::from_parts_with_source(
        0.0,
        duration,
        text.clone(),
        TimestampSource::SyntheticSpan,
    )];
    (text, segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn mode_parse() {
        assert_eq!(
            OpenRouterSttMode::parse("auto").unwrap(),
            OpenRouterSttMode::Auto
        );
        assert_eq!(
            OpenRouterSttMode::parse("transcriptions").unwrap(),
            OpenRouterSttMode::Transcriptions
        );
        assert!(OpenRouterSttMode::parse("nope").is_err());
    }

    #[test]
    fn dedicated_registry_lookup() {
        // Registry-authoritative: only reviewed ASR ids, not name substrings.
        assert!(looks_like_dedicated_asr("openai/whisper-1"));
        assert!(looks_like_dedicated_asr("openai/gpt-4o-transcribe"));
        assert!(!looks_like_dedicated_asr("google/gemini-2.5-flash"));
        assert!(!looks_like_dedicated_asr(
            "vendor/whisper-clone-experimental"
        ));
    }

    #[tokio::test]
    async fn missing_key_fails_early() {
        let err = OpenRouterProvider::new(None, None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::User(UserError::MissingApiKey)
        ));
    }

    #[tokio::test]
    async fn parses_successful_chat_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": { "content": "Hello from the cloud." }
                }]
            })))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_policy(
            Some("test-key".into()),
            Some(server.uri()),
            RemotePolicy {
                allow_loopback_http: true,
                ..Default::default()
            },
            OpenRouterSttMode::Chat,
        )
        .unwrap();

        let samples: Arc<[f32]> = vec![0.0f32; 1600].into();
        let input =
            AudioInput::from_parts_unchecked(PathBuf::from("silent.wav"), samples, 16_000, 0.1);
        let opts = TranscriptionOptions {
            model: "google/gemini-2.5-flash".into(),
            language: "en".into(),
            timestamps: false,
            cancel: None,
            op: None,
        };
        let result = provider.transcribe(&input, &opts).await.unwrap();
        assert_eq!(result.text(), "Hello from the cloud.");
        assert_eq!(result.provider(), "openrouter");
        assert_eq!(result.backend_kind(), BackendKind::LlmAssisted);
        assert!(!result.timestamps_reliable());
    }

    #[tokio::test]
    async fn dedicated_path_hits_transcriptions() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "text": "Dedicated ASR path works."
            })))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_policy(
            Some("test-key".into()),
            Some(server.uri()),
            RemotePolicy {
                allow_loopback_http: true,
                ..Default::default()
            },
            OpenRouterSttMode::Transcriptions,
        )
        .unwrap();

        let input = AudioInput::from_parts_unchecked(
            PathBuf::from("x.wav"),
            vec![0.0; 1600].into(),
            16_000,
            0.1,
        );
        let opts = TranscriptionOptions {
            model: "openai/whisper-1".into(),
            language: "en".into(),
            timestamps: false,
            cancel: None,
            op: None,
        };
        let result = provider.transcribe(&input, &opts).await.unwrap();
        assert_eq!(result.text(), "Dedicated ASR path works.");
        assert_eq!(result.backend_kind(), BackendKind::Asr);
    }

    #[tokio::test]
    async fn maps_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_policy(
            Some("test-key".into()),
            Some(server.uri()),
            RemotePolicy {
                allow_loopback_http: true,
                ..Default::default()
            },
            OpenRouterSttMode::Chat,
        )
        .unwrap();
        let input = AudioInput::from_parts_unchecked(
            PathBuf::from("x.wav"),
            vec![0.0; 1600].into(),
            16_000,
            0.1,
        );
        let opts = TranscriptionOptions {
            model: "google/gemini-2.5-flash".into(),
            language: "auto".into(),
            timestamps: false,
            cancel: None,
            op: None,
        };
        let err = provider.transcribe(&input, &opts).await.unwrap_err();
        match err {
            crate::error::TranscriptionError::Provider(ProviderError::RateLimited { .. }) => {}
            other => panic!("expected rate limit, got {other}"),
        }
    }

    #[test]
    fn parse_timestamp_json() {
        let raw = r#"{"text":"Hi there","segments":[{"start":0.0,"end":1.0,"text":"Hi there"}]}"#;
        let (text, segs) = parse_chat_content(raw, true, 1.0);
        assert_eq!(text, "Hi there");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].end, 1.0);
    }

    #[test]
    fn auto_routes_whisper_to_transcriptions() {
        let p = OpenRouterProvider::with_policy(
            Some("k".into()),
            Some("https://openrouter.ai/api/v1".into()),
            RemotePolicy::default(),
            OpenRouterSttMode::Auto,
        )
        .unwrap();
        assert_eq!(
            p.resolve_path("openai/whisper-large-v3").unwrap(),
            SttPath::Transcriptions
        );
        assert_eq!(
            p.resolve_path("google/gemini-2.5-flash").unwrap(),
            SttPath::Chat
        );
    }

    #[test]
    fn auto_unknown_model_fails_closed() {
        let p = OpenRouterProvider::with_policy(
            Some("k".into()),
            Some("https://openrouter.ai/api/v1".into()),
            RemotePolicy::default(),
            OpenRouterSttMode::Auto,
        )
        .unwrap();
        let err = p.resolve_path("acme/unknown-model-v1").unwrap_err();
        assert!(
            err.to_string().contains("reviewed") || err.to_string().contains("unsupported"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn explicit_transcriptions_accepts_unregistered() {
        let p = OpenRouterProvider::with_policy(
            Some("k".into()),
            Some("https://openrouter.ai/api/v1".into()),
            RemotePolicy::default(),
            OpenRouterSttMode::Transcriptions,
        )
        .unwrap();
        assert_eq!(
            p.resolve_path("vendor/custom-asr").unwrap(),
            SttPath::Transcriptions
        );
    }
}
