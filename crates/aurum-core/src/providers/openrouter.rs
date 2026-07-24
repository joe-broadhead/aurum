//! OpenRouter remote transcription provider.
//!
//! OpenRouter does not currently expose an OpenAI-compatible
//! `/audio/transcriptions` endpoint. Instead, audio is sent via the
//! multimodal chat completions API (`input_audio`).
//!
//! **Important product semantics:** this is LLM-assisted transcription, not a
//! dedicated ASR model. Output may paraphrase, omit filler, or invent
//! timestamps. Prefer the local provider when verbatim accuracy matters.

use super::{
    BackendKind, Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult,
};
use crate::audio::{self, AudioInput, DEFAULT_MAX_UPLOAD_BYTES};
use crate::error::{ProviderError, Result, UserError};
use crate::postprocess::{self, truncate_chars};
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
const PROVIDER_NAME: &str = "openrouter";

/// OpenRouter provider using multimodal chat completions for transcription.
pub struct OpenRouterProvider {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
    max_upload_bytes: usize,
}

impl std::fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("base_url", &self.base_url)
            .field("api_key", &"***")
            .field("max_upload_bytes", &self.max_upload_bytes)
            .finish()
    }
}

impl OpenRouterProvider {
    /// Create a provider. Fails early if the API key is missing/empty.
    pub fn new(api_key: Option<String>, base_url: Option<String>) -> Result<Self> {
        let api_key = api_key
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(UserError::MissingApiKey)?;

        let base_url = base_url
            .map(|s| s.trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let http = reqwest::Client::builder()
            .user_agent(concat!("aurum/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| ProviderError::Network {
                provider: PROVIDER_NAME.into(),
                reason: e.to_string(),
            })?;

        Ok(Self {
            api_key,
            base_url,
            http,
            max_upload_bytes: DEFAULT_MAX_UPLOAD_BYTES,
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl TranscriptionProvider for OpenRouterProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::LlmAssisted
    }

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        // Compress before base64 — raw WAV in JSON OOMs on long audio.
        let (upload_path, format) =
            audio::encode_for_upload(&input.samples, self.max_upload_bytes).await?;
        let cleanup = scopeguard_path(upload_path.clone());

        let file_bytes = tokio::fs::read(&upload_path).await?;
        if file_bytes.len() > self.max_upload_bytes {
            return Err(UserError::AudioTooLarge {
                decoded_bytes: file_bytes.len(),
                max_bytes: self.max_upload_bytes,
            }
            .into());
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
        drop(cleanup); // remove temp file as soon as encoded

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
        });

        tracing::debug!(
            model = %options.model,
            url = %self.chat_url(),
            upload_format = format,
            upload_bytes = file_bytes.len(),
            "openrouter request"
        );

        let response = self
            .http
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/joe-broadhead/aurum")
            .header("X-Title", "Aurum")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                provider: PROVIDER_NAME.into(),
                reason: e.to_string(),
            })?;

        let status = response.status();
        let body_text = response.text().await.map_err(|e| ProviderError::Network {
            provider: PROVIDER_NAME.into(),
            reason: e.to_string(),
        })?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::Auth {
                provider: PROVIDER_NAME.into(),
                reason: truncate_chars(&body_text, 300),
            }
            .into());
        }
        if status.as_u16() == 429 {
            return Err(ProviderError::RateLimited {
                provider: PROVIDER_NAME.into(),
            }
            .into());
        }
        if status.as_u16() == 402 {
            return Err(ProviderError::QuotaExceeded {
                provider: PROVIDER_NAME.into(),
                reason: truncate_chars(&body_text, 300),
            }
            .into());
        }
        if !status.is_success() {
            return Err(ProviderError::Remote {
                provider: PROVIDER_NAME.into(),
                reason: format!("HTTP {status}: {}", truncate_chars(&body_text, 500)),
            }
            .into());
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body_text).map_err(|e| ProviderError::Remote {
                provider: PROVIDER_NAME.into(),
                reason: format!(
                    "invalid JSON response: {e}; body={}",
                    truncate_chars(&body_text, 300)
                ),
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

        let (text, segments) = parse_content(&content, options.timestamps, input.duration_secs);

        let result = TranscriptionResult::openrouter(
            text,
            segments,
            if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            },
            options.model.clone(),
            input.duration_secs,
            options.timestamps,
        );

        Ok(postprocess::normalize_result(result))
    }
}

/// Tiny RAII helper so temp upload files are removed even on early return.
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

fn parse_content(content: &str, want_timestamps: bool, duration: f64) -> (String, Vec<Segment>) {
    if want_timestamps {
        let cleaned = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        if let Ok(payload) = serde_json::from_str::<TimestampPayload>(cleaned) {
            return (payload.text, payload.segments);
        }
    }

    let text = content.to_string();
    let segments = vec![Segment {
        start: 0.0,
        end: duration,
        text: text.clone(),
    }];
    (text, segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn missing_key_fails_early() {
        let err = OpenRouterProvider::new(None, None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::TranscriptionError::User(UserError::MissingApiKey)
        ));
    }

    #[tokio::test]
    async fn parses_successful_response() {
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

        let provider =
            OpenRouterProvider::new(Some("test-key".into()), Some(server.uri())).unwrap();

        let samples: Arc<[f32]> = vec![0.0f32; 1600].into();
        let input = AudioInput {
            source_path: PathBuf::from("silent.wav"),
            samples,
            sample_rate: 16_000,
            duration_secs: 0.1,
        };
        let opts = TranscriptionOptions {
            model: "google/gemini-2.5-flash".into(),
            language: "en".into(),
            timestamps: false,
        };
        let result = provider.transcribe(&input, &opts).await.unwrap();
        assert_eq!(result.text, "Hello from the cloud.");
        assert_eq!(result.provider, "openrouter");
        assert_eq!(result.language.as_deref(), Some("en"));
    }

    #[tokio::test]
    async fn maps_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .mount(&server)
            .await;

        let provider =
            OpenRouterProvider::new(Some("test-key".into()), Some(server.uri())).unwrap();
        let input = AudioInput {
            source_path: PathBuf::from("x.wav"),
            samples: vec![0.0; 1600].into(),
            sample_rate: 16_000,
            duration_secs: 0.1,
        };
        let opts = TranscriptionOptions {
            model: "google/gemini-2.5-flash".into(),
            language: "auto".into(),
            timestamps: false,
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
        let (text, segs) = parse_content(raw, true, 1.0);
        assert_eq!(text, "Hi there");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].end, 1.0);
    }
}
