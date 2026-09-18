//! CLI-only chat completions for `aurum converse` (GitHub #140 P3).
//!
//! Not part of `aurum-core`. Uses [`aurum_core::remote::HardenedHttpClient`] on
//! official OpenAI / OpenRouter / xAI origins. Never selected just because a
//! key is present.

use aurum_core::error::{ProviderError, Result, UserError};
use aurum_core::remote::{
    map_http_status, read_body_limited, HardenedHttpClient, OpenAiHttpPolicy, OpenRouterHttpPolicy,
    RemoteBodyLimits, RemotePolicy, XaiHttpPolicy,
};
use aurum_core::secret::SecretString;
use serde_json::{json, Value};

const DEFAULT_OPENAI_LLM: &str = "gpt-4o-mini";
const DEFAULT_OPENROUTER_LLM: &str = "google/gemini-2.5-flash-lite";
const DEFAULT_SYSTEM: &str = "You are a concise voice assistant. Reply in plain spoken English. \
No markdown, no lists, no URLs unless asked. Prefer one or two short sentences (under 40 words).";

/// Explicit converse LLM backend (never inferred from env keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAi,
    OpenRouter,
    Xai,
}

impl LlmProvider {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "openai" => Ok(Self::OpenAi),
            "openrouter" => Ok(Self::OpenRouter),
            "xai" | "grok" => Ok(Self::Xai),
            "none" | "" => Err(UserError::Other {
                message: "llm provider 'none' means use --reply-text / --reply-file instead".into(),
            }
            .into()),
            "elevenlabs" | "local" => Err(UserError::Other {
                message: format!(
                    "unsupported --llm-provider '{s}'\n  Hint: use openai, openrouter, or xai"
                ),
            }
            .into()),
            other => Err(UserError::InvalidProvider {
                provider: other.into(),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::Xai => "xai",
        }
    }

    /// Default chat model when `--llm-model` is omitted.
    ///
    /// xAI has no in-repo reviewed chat id — require `--llm-model`.
    pub fn default_model(self) -> Result<&'static str> {
        match self {
            Self::OpenAi => Ok(DEFAULT_OPENAI_LLM),
            Self::OpenRouter => Ok(DEFAULT_OPENROUTER_LLM),
            Self::Xai => Err(UserError::Other {
                message: "xAI chat requires --llm-model (no default chat id in Aurum)\n  \
                     Hint: pass a Grok chat model id from your xAI account"
                    .into(),
            }
            .into()),
        }
    }
}

pub struct ChatRequest<'a> {
    pub provider: LlmProvider,
    pub model: &'a str,
    pub api_key: &'a SecretString,
    pub base_url: Option<&'a str>,
    pub user_text: &'a str,
    pub system: &'a str,
    /// Prior turns as (role, content), excluding the new user text.
    pub prior: &'a [(&'a str, &'a str)],
}

/// One-shot chat completion. Non-streaming (full reply then TTS).
pub async fn complete_chat(req: ChatRequest<'_>) -> Result<String> {
    let model = req.model.trim();
    if model.is_empty() {
        return Err(UserError::InvalidConfig {
            reason: "llm model must be non-empty".into(),
        }
        .into());
    }
    let user = req.user_text.trim();
    if user.is_empty() {
        return Err(UserError::Other {
            message: "cannot call LLM with an empty transcript".into(),
        }
        .into());
    }
    let key = req.api_key.expose();
    if key.trim().is_empty() {
        return Err(UserError::MissingProviderCredential {
            provider: req.provider.as_str().into(),
        }
        .into());
    }

    let loopback = req
        .base_url
        .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"));
    let policy = RemotePolicy {
        allow_loopback_http: loopback,
        ..RemotePolicy::default()
    };
    let http = match req.provider {
        LlmProvider::OpenAi => HardenedHttpClient::build(req.base_url, policy, OpenAiHttpPolicy)?,
        LlmProvider::OpenRouter => {
            HardenedHttpClient::build(req.base_url, policy, OpenRouterHttpPolicy)?
        }
        LlmProvider::Xai => HardenedHttpClient::build(req.base_url, policy, XaiHttpPolicy)?,
    };

    let system = if req.system.trim().is_empty() {
        DEFAULT_SYSTEM
    } else {
        req.system.trim()
    };
    let mut messages = Vec::with_capacity(2 + req.prior.len());
    messages.push(json!({ "role": "system", "content": system }));
    for (role, content) in req.prior {
        messages.push(json!({ "role": role, "content": content }));
    }
    messages.push(json!({ "role": "user", "content": user }));
    let body = chat_body(model, &messages, false);
    let response = http
        .request(reqwest::Method::POST, "chat/completions", key)?
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Network {
            provider: req.provider.as_str().into(),
            reason: e.to_string(),
        })?;

    let status = response.status();
    let bytes =
        read_body_limited(response, req.provider.as_str(), RemoteBodyLimits::chat()).await?;
    let body_text = String::from_utf8_lossy(&bytes).into_owned();
    map_http_status(req.provider.as_str(), status, &body_text)?;

    extract_assistant_text(&body_text, req.provider.as_str())
}

fn chat_body(model: &str, messages: &[Value], stream: bool) -> Value {
    json!({
        "model": model,
        "temperature": 0.4,
        "max_tokens": 160,
        "stream": stream,
        "messages": messages,
    })
}

fn build_messages(req: &ChatRequest<'_>) -> (String, Vec<Value>) {
    let model = req.model.trim().to_string();
    let system = if req.system.trim().is_empty() {
        DEFAULT_SYSTEM
    } else {
        req.system.trim()
    };
    let mut messages = Vec::with_capacity(2 + req.prior.len());
    messages.push(json!({ "role": "system", "content": system }));
    for (role, content) in req.prior {
        messages.push(json!({ "role": role, "content": content }));
    }
    messages.push(json!({ "role": "user", "content": req.user_text.trim() }));
    (model, messages)
}

/// Streaming chat. `on_delta` is called with each text fragment. Returns the full reply.
pub async fn stream_chat(req: ChatRequest<'_>, mut on_delta: impl FnMut(&str)) -> Result<String> {
    let user = req.user_text.trim();
    if user.is_empty() {
        return Err(UserError::Other {
            message: "cannot call LLM with an empty transcript".into(),
        }
        .into());
    }
    let key = req.api_key.expose();
    if key.trim().is_empty() {
        return Err(UserError::MissingProviderCredential {
            provider: req.provider.as_str().into(),
        }
        .into());
    }
    let model = req.model.trim();
    if model.is_empty() {
        return Err(UserError::InvalidConfig {
            reason: "llm model must be non-empty".into(),
        }
        .into());
    }

    let loopback = req
        .base_url
        .is_some_and(|u| u.contains("127.0.0.1") || u.contains("localhost"));
    let policy = RemotePolicy {
        allow_loopback_http: loopback,
        ..RemotePolicy::default()
    };
    let http = match req.provider {
        LlmProvider::OpenAi => HardenedHttpClient::build(req.base_url, policy, OpenAiHttpPolicy)?,
        LlmProvider::OpenRouter => {
            HardenedHttpClient::build(req.base_url, policy, OpenRouterHttpPolicy)?
        }
        LlmProvider::Xai => HardenedHttpClient::build(req.base_url, policy, XaiHttpPolicy)?,
    };
    let (model, messages) = {
        let (m, msgs) = build_messages(&req);
        (m, msgs)
    };
    let body = chat_body(&model, &messages, true);
    let mut response = http
        .request(reqwest::Method::POST, "chat/completions", key)?
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Network {
            provider: req.provider.as_str().into(),
            reason: e.to_string(),
        })?;
    let status = response.status();
    if !status.is_success() {
        let bytes =
            read_body_limited(response, req.provider.as_str(), RemoteBodyLimits::chat()).await?;
        let body_text = String::from_utf8_lossy(&bytes).into_owned();
        map_http_status(req.provider.as_str(), status, &body_text)?;
        return Err(ProviderError::Remote {
            provider: req.provider.as_str().into(),
            reason: body_text,
        }
        .into());
    }

    let mut rest = String::new();
    let mut full = String::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| ProviderError::Network {
        provider: req.provider.as_str().into(),
        reason: e.to_string(),
    })? {
        rest.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = rest.find('\n') {
            let mut line: String = rest[..idx].to_string();
            rest = rest[idx + 1..].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(full);
            }
            if let Some(piece) = delta_content(data) {
                on_delta(&piece);
                full.push_str(&piece);
            }
        }
    }
    if full.trim().is_empty() {
        return Err(ProviderError::InvalidProviderPayload {
            provider: req.provider.as_str().into(),
            reason: "streaming chat completion had empty assistant content".into(),
        }
        .into());
    }
    Ok(full)
}

fn delta_content(data: &str) -> Option<String> {
    let v: Value = serde_json::from_str(data).ok()?;
    let content = v
        .get("choices")?
        .as_array()?
        .first()?
        .get("delta")?
        .get("content")?;
    match content {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Pull a speakable clause off `buf` (sentence end, or a long comma/space split).
pub fn take_speakable(buf: &mut String) -> Option<String> {
    const MIN: usize = 12;
    const FORCE: usize = 90;
    let chars: Vec<char> = buf.chars().collect();
    if chars.len() < MIN {
        return None;
    }
    let mut split_at: Option<usize> = None;
    for (i, c) in chars.iter().enumerate() {
        if matches!(*c, '.' | '!' | '?') && i + 1 >= MIN {
            split_at = Some(i + 1);
            break;
        }
    }
    if split_at.is_none() && chars.len() >= FORCE {
        if let Some(i) = chars
            .iter()
            .take(FORCE)
            .rposition(|c| *c == ' ' || *c == ',')
        {
            if i >= MIN {
                split_at = Some(i + 1);
            }
        }
    }
    let n = split_at?;
    let (head, tail): (String, String) = {
        let head: String = chars.iter().take(n).collect();
        let tail: String = chars.iter().skip(n).collect();
        (head, tail)
    };
    let head = head.trim().to_string();
    *buf = tail.trim_start().to_string();
    if head.is_empty() {
        None
    } else {
        Some(head)
    }
}

fn extract_assistant_text(body: &str, provider: &str) -> Result<String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: format!("invalid JSON: {e}"),
        })?;
    let content = v
        .get("choices")
        .and_then(|c| c.as_array().and_then(|a| a.first()))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"));
    let text = match content {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string(),
        _ => String::new(),
    };
    if text.is_empty() {
        return Err(ProviderError::InvalidProviderPayload {
            provider: provider.into(),
            reason: "chat completion had empty assistant content".into(),
        }
        .into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parse_providers() {
        assert_eq!(LlmProvider::parse("openai").unwrap(), LlmProvider::OpenAi);
        assert_eq!(
            LlmProvider::parse("OPENROUTER").unwrap(),
            LlmProvider::OpenRouter
        );
        assert_eq!(LlmProvider::parse("grok").unwrap(), LlmProvider::Xai);
        assert!(LlmProvider::parse("elevenlabs").is_err());
        assert!(LlmProvider::parse("local").is_err());
        assert!(LlmProvider::parse("none").is_err());
    }

    #[test]
    fn xai_requires_model() {
        assert!(LlmProvider::Xai.default_model().is_err());
        assert_eq!(LlmProvider::OpenAi.default_model().unwrap(), "gpt-4o-mini");
    }

    #[tokio::test]
    async fn mock_openai_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "content": "  Hello there.  " } }]
            })))
            .mount(&server)
            .await;

        let key = SecretString::new("sk-test");
        let text = complete_chat(ChatRequest {
            provider: LlmProvider::OpenAi,
            model: "gpt-4o-mini",
            api_key: &key,
            base_url: Some(&server.uri()),
            user_text: "hi",
            system: DEFAULT_SYSTEM,
            prior: &[],
        })
        .await
        .unwrap();
        assert_eq!(text, "Hello there.");
    }

    #[test]
    fn speakable_splits_sentences() {
        let mut b = String::from("Hello there. More");
        assert_eq!(take_speakable(&mut b).as_deref(), Some("Hello there."));
        assert_eq!(b, "More");
        assert!(take_speakable(&mut b).is_none());
    }

    #[tokio::test]
    async fn mock_openai_stream() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"there.\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;
        let key = SecretString::new("sk-test");
        let mut pieces = Vec::new();
        let full = stream_chat(
            ChatRequest {
                provider: LlmProvider::OpenAi,
                model: "gpt-4o-mini",
                api_key: &key,
                base_url: Some(&server.uri()),
                user_text: "hi",
                system: DEFAULT_SYSTEM,
                prior: &[],
            },
            |d| pieces.push(d.to_string()),
        )
        .await
        .unwrap();
        assert_eq!(full, "Hello there.");
        assert_eq!(pieces.concat(), full);
    }
}
