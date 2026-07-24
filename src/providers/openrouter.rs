//! OpenRouter remote transcription provider.
//!
//! OpenRouter does not currently expose an OpenAI-compatible
//! `/audio/transcriptions` endpoint. Instead, audio is sent via the
//! multimodal chat completions API (`input_audio`). This provider wraps that
//! flow and normalizes the response into [`TranscriptionResult`].

use super::{Segment, TranscriptionOptions, TranscriptionProvider, TranscriptionResult};
use crate::audio::{self, AudioInput};
use crate::error::{ProviderError, Result, UserError};
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
}

impl std::fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("base_url", &self.base_url)
            .field("api_key", &"***")
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

    async fn transcribe(
        &self,
        input: &AudioInput,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult> {
        // Build a temp WAV so we have a well-defined format for base64 upload.
        let tmp_dir = std::env::temp_dir().join(format!("aurum-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).ok();
        let wav_path: PathBuf = tmp_dir.join("upload.wav");
        audio::write_temp_wav(&input.samples, &wav_path)?;

        let wav_bytes = tokio::fs::read(&wav_path).await?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

        // Best-effort cleanup.
        let _ = std::fs::remove_file(&wav_path);

        let mut prompt =
            String::from("Transcribe the audio verbatim. Reply with ONLY the transcript text");
        if options.timestamps {
            prompt.push_str(
                ", as a JSON object with keys \"text\" (string) and \"segments\" \
                 (array of {\"start\": number, \"end\": number, \"text\": string}) \
                 where times are in seconds. Do not wrap in markdown.",
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
                            "format": "wav"
                        }
                    }
                ]
            }],
            "temperature": 0,
        });

        tracing::debug!(model = %options.model, url = %self.chat_url(), "openrouter request");

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
                reason: truncate(&body_text, 300),
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
                reason: truncate(&body_text, 300),
            }
            .into());
        }
        if !status.is_success() {
            return Err(ProviderError::Remote {
                provider: PROVIDER_NAME.into(),
                reason: format!("HTTP {status}: {}", truncate(&body_text, 500)),
            }
            .into());
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body_text).map_err(|e| ProviderError::Remote {
                provider: PROVIDER_NAME.into(),
                reason: format!(
                    "invalid JSON response: {e}; body={}",
                    truncate(&body_text, 300)
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

        Ok(TranscriptionResult {
            text,
            segments,
            language: if lang != "auto" && !lang.is_empty() {
                Some(lang)
            } else {
                None
            },
            model: options.model.clone(),
            provider: PROVIDER_NAME.to_string(),
            duration_secs: input.duration_secs,
        })
    }
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
        // Strip optional markdown fences.
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

    // Plain text fallback — single segment spanning the full duration.
    let text = content.to_string();
    let segments = vec![Segment {
        start: 0.0,
        end: duration,
        text: text.clone(),
    }];
    (text, segments)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        // 0.1s of silence
        let samples = vec![0.0f32; 1600];
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
            samples: vec![0.0; 1600],
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
