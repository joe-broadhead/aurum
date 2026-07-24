//! LLM-assisted cleanup via OpenRouter chat completions.

use super::{CleanupProviderKind, CleanupResult, CleanupStyle, TextCleanup};
use crate::error::{ProviderError, Result, UserError};
use crate::postprocess::truncate_chars;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MODEL: &str = "google/gemini-2.5-flash";

/// OpenRouter-backed text cleanup (explicit opt-in; not local-first).
pub struct OpenRouterCleanup {
    api_key: String,
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl OpenRouterCleanup {
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Result<Self> {
        let api_key = api_key
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(UserError::MissingApiKey)?;
        let base_url = base_url
            .map(|s| s.trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = model
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let http = reqwest::Client::builder()
            .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Network {
                provider: "openrouter-cleanup".into(),
                reason: e.to_string(),
            })?;
        Ok(Self {
            api_key,
            base_url,
            model,
            http,
        })
    }

    fn prompt(style: CleanupStyle, text: &str) -> String {
        let instruction = match style {
            CleanupStyle::Raw => "Return the text unchanged.",
            CleanupStyle::Clean => {
                "Clean up this speech transcript: remove filler words (um, uh, you know), \
                 fix spacing/punctuation, keep meaning verbatim. Reply with ONLY the cleaned text."
            }
            CleanupStyle::Bullets => {
                "Turn this transcript into a concise bullet list. One bullet per idea. \
                 Reply with ONLY the bullets, each starting with • ."
            }
            CleanupStyle::Professional => {
                "Rewrite this transcript in clear professional prose. Expand casualisms, \
                 keep facts. Reply with ONLY the rewritten text."
            }
            CleanupStyle::Summary => {
                "Summarize this transcript in 1-3 short sentences. Reply with ONLY the summary."
            }
        };
        format!("{instruction}\n\n---\n{text}\n---")
    }
}

#[async_trait]
impl TextCleanup for OpenRouterCleanup {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    fn kind(&self) -> CleanupProviderKind {
        CleanupProviderKind::OpenRouter
    }

    async fn cleanup(&self, text: &str, style: CleanupStyle) -> Result<CleanupResult> {
        let original = text.to_string();
        if text.trim().is_empty() || matches!(style, CleanupStyle::Raw) {
            return Ok(CleanupResult {
                text: text.trim().to_string(),
                style,
                provider: CleanupProviderKind::OpenRouter,
                original_text: original,
            });
        }

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": Self::prompt(style, text)}
            ],
            "temperature": 0.2,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/joe-broadhead/aurum")
            .header("X-Title", "Aurum Cleanup")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                provider: "openrouter-cleanup".into(),
                reason: e.to_string(),
            })?;

        let status = response.status();
        let body_text = response.text().await.map_err(|e| ProviderError::Network {
            provider: "openrouter-cleanup".into(),
            reason: e.to_string(),
        })?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::Auth {
                provider: "openrouter-cleanup".into(),
                reason: truncate_chars(&body_text, 300),
            }
            .into());
        }
        if status.as_u16() == 429 {
            return Err(ProviderError::RateLimited {
                provider: "openrouter-cleanup".into(),
            }
            .into());
        }
        if !status.is_success() {
            return Err(ProviderError::Remote {
                provider: "openrouter-cleanup".into(),
                reason: format!("HTTP {status}: {}", truncate_chars(&body_text, 400)),
            }
            .into());
        }

        let parsed: ChatResponse =
            serde_json::from_str(&body_text).map_err(|e| ProviderError::Remote {
                provider: "openrouter-cleanup".into(),
                reason: format!("invalid JSON: {e}"),
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
                reason: "cleanup model returned empty text".into(),
            }
            .into());
        }

        Ok(CleanupResult {
            text: content,
            style,
            provider: CleanupProviderKind::OpenRouter,
            original_text: original,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Debug, Deserialize)]
struct Choice {
    message: Msg,
}
#[derive(Debug, Deserialize)]
struct Msg {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn missing_key() {
        assert!(OpenRouterCleanup::new(None, None, None).is_err());
    }

    #[tokio::test]
    async fn cleans_via_mock() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "Hello world."}}]
            })))
            .mount(&server)
            .await;

        let c = OpenRouterCleanup::new(
            Some("k".into()),
            Some(server.uri()),
            Some("test/model".into()),
        )
        .unwrap();
        let out = c
            .cleanup("um hello world", CleanupStyle::Clean)
            .await
            .unwrap();
        assert_eq!(out.text, "Hello world.");
        assert_eq!(out.provider, CleanupProviderKind::OpenRouter);
    }
}
