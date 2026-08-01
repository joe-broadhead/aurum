//! Policy-enforcing HTTP client for remote STT and cleanup (JOE-1587).

use crate::error::{ProviderError, Result, UserError};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use std::time::Duration;
use url::Url;

/// Official OpenRouter HTTPS origin (credentialed default).
pub const DEFAULT_OPENROUTER_ORIGIN: &str = "https://openrouter.ai";

/// Validated remote endpoint with trust classification.
#[derive(Debug, Clone)]
pub struct RemoteEndpoint {
    pub base_url: String,
    /// True when this is the official OpenRouter origin.
    pub is_official: bool,
    /// True when credentials may be sent (official or explicit custom trust).
    pub credentials_allowed: bool,
}

/// Policy knobs for building the shared client.
#[derive(Debug, Clone)]
pub struct RemotePolicy {
    /// Connect timeout.
    pub connect_timeout: Duration,
    /// Total request timeout.
    pub total_timeout: Duration,
    /// When false (default), system proxy is not used.
    pub use_system_proxy: bool,
    /// When true, allow custom non-OpenRouter HTTPS endpoints with credentials
    /// (requires separate config opt-in).
    pub allow_custom_credentialed_endpoint: bool,
    /// When true, allow HTTP only for loopback hosts (tests).
    pub allow_loopback_http: bool,
}

impl Default for RemotePolicy {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(600),
            use_system_proxy: false,
            allow_custom_credentialed_endpoint: false,
            allow_loopback_http: false,
        }
    }
}

/// Parse and validate a base URL under the remote policy.
pub fn validate_endpoint(raw: &str, policy: &RemotePolicy) -> Result<RemoteEndpoint> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(UserError::InvalidConfig {
            reason: "remote base URL is empty".into(),
        }
        .into());
    }
    let url = Url::parse(trimmed).map_err(|e| UserError::InvalidConfig {
        reason: format!("invalid remote base URL: {e}"),
    })?;

    if url.username() != "" || url.password().is_some() {
        return Err(UserError::InvalidConfig {
            reason: "remote base URL must not embed userinfo/credentials".into(),
        }
        .into());
    }

    let scheme = url.scheme();
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let is_loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    let is_official = host == "openrouter.ai" && scheme == "https";

    match scheme {
        "https" => {}
        "http" if policy.allow_loopback_http && is_loopback => {}
        "http" => {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "HTTP remote endpoints are only allowed for loopback test mode (got {trimmed})"
                ),
            }
            .into());
        }
        other => {
            return Err(UserError::InvalidConfig {
                reason: format!("unsupported URL scheme '{other}' (use https)"),
            }
            .into());
        }
    }

    let credentials_allowed = is_official
        || (is_loopback && policy.allow_loopback_http)
        || (policy.allow_custom_credentialed_endpoint && scheme == "https");
    if !credentials_allowed {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "credentialed remote endpoint '{trimmed}' is not the official OpenRouter origin.\n  \
                 Hint: set openrouter.allow_custom_endpoint = true only for trusted compatible APIs, \
                 or use {DEFAULT_OPENROUTER_ORIGIN}."
            ),
        }
        .into());
    }

    Ok(RemoteEndpoint {
        base_url: trimmed.to_string(),
        is_official,
        credentials_allowed,
    })
}

/// Hardened reqwest client shared by STT and cleanup.
#[derive(Clone)]
pub struct HardenedHttpClient {
    http: Client,
    endpoint: RemoteEndpoint,
    policy: RemotePolicy,
}

impl std::fmt::Debug for HardenedHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HardenedHttpClient")
            .field("base_url", &self.endpoint.base_url)
            .field("is_official", &self.endpoint.is_official)
            .finish()
    }
}

impl HardenedHttpClient {
    pub fn build(base_url: Option<&str>, policy: RemotePolicy) -> Result<Self> {
        let raw = base_url
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("https://openrouter.ai/api/v1");
        let endpoint = validate_endpoint(raw, &policy)?;

        let mut builder = Client::builder()
            .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(policy.connect_timeout)
            .timeout(policy.total_timeout)
            // Never follow redirects with credentials (JOE-1587).
            .redirect(reqwest::redirect::Policy::none());

        if !policy.use_system_proxy {
            builder = builder.no_proxy();
        }

        let http = builder.build().map_err(|e| ProviderError::Network {
            provider: "remote".into(),
            reason: e.to_string(),
        })?;

        Ok(Self {
            http,
            endpoint,
            policy,
        })
    }

    pub fn endpoint(&self) -> &RemoteEndpoint {
        &self.endpoint
    }

    pub fn base_url(&self) -> &str {
        &self.endpoint.base_url
    }

    pub fn policy(&self) -> &RemotePolicy {
        &self.policy
    }

    /// Build a request with auth + standard OpenRouter headers.
    pub fn request(&self, method: Method, path: &str, api_key: &str) -> Result<RequestBuilder> {
        if !self.endpoint.credentials_allowed {
            return Err(UserError::InvalidConfig {
                reason: "credentials are not allowed for this endpoint under current policy".into(),
            }
            .into());
        }
        let path = path.trim_start_matches('/');
        let url = format!("{}/{}", self.endpoint.base_url, path);
        // Reject if path somehow rewrites host (defense in depth).
        if let Ok(u) = Url::parse(&url) {
            let base = Url::parse(&self.endpoint.base_url).ok();
            if let Some(b) = base {
                if u.origin() != b.origin() {
                    return Err(UserError::InvalidConfig {
                        reason: "request URL origin diverged from validated endpoint".into(),
                    }
                    .into());
                }
            }
        }
        Ok(self
            .http
            .request(method, url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("HTTP-Referer", "https://github.com/joe-broadhead/aurum")
            .header("X-Title", "Aurum")
            .header(
                "X-Request-Id",
                format!(
                    "aurum-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                ),
            ))
    }

    pub fn get_raw(&self) -> &Client {
        &self.http
    }
}

/// Map HTTP status codes to typed provider errors.
///
/// Public reasons are **allowlisted only** (HTTP status + optional provider code).
/// Arbitrary remote response bodies are never echoed (JOE-1914).
pub fn map_http_status(provider: &str, status: StatusCode, body: &str) -> Result<()> {
    use super::status::public_http_reason;
    let code = status.as_u16();
    let reason = public_http_reason(code, body);
    match code {
        200..=299 => Ok(()),
        401 | 403 => Err(ProviderError::Auth {
            provider: provider.into(),
            reason,
        }
        .into()),
        429 => Err(ProviderError::RateLimited {
            provider: provider.into(),
        }
        .into()),
        402 => Err(ProviderError::QuotaExceeded {
            provider: provider.into(),
            reason,
        }
        .into()),
        _ => Err(ProviderError::Remote {
            provider: provider.into(),
            reason,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_endpoint_ok() {
        let ep =
            validate_endpoint("https://openrouter.ai/api/v1", &RemotePolicy::default()).unwrap();
        assert!(ep.is_official);
        assert!(ep.credentials_allowed);
    }

    #[test]
    fn foreign_host_rejected_by_default() {
        let err =
            validate_endpoint("https://evil.example/api", &RemotePolicy::default()).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(
            err.to_string().contains("allow_custom_endpoint")
                || err.to_string().contains("official")
        );
    }

    #[test]
    fn map_http_status_never_echoes_body_payload() {
        let body = r#"{"error":{"message":"sk-or-v1-canary-should-not-appear","code":401}}"#;
        let err =
            map_http_status("openrouter", reqwest::StatusCode::UNAUTHORIZED, body).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("canary"));
        assert!(!msg.contains("sk-or-v1"));
        assert!(msg.contains("401") || msg.to_ascii_lowercase().contains("auth"));
    }

    #[test]
    fn map_http_status_remote_allowlists_code_only() {
        let body =
            r#"{"error":{"message":"transcript: hello world secret","code":"no_endpoints"}}"#;
        let err = map_http_status("openrouter", reqwest::StatusCode::NOT_FOUND, body).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("transcript"));
        assert!(!msg.contains("hello world"));
        assert!(msg.contains("404"));
        assert!(msg.contains("no_endpoints"));
    }

    #[test]
    fn custom_allowed_with_opt_in() {
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: true,
            ..Default::default()
        };
        let ep = validate_endpoint("https://compatible.example/v1", &policy).unwrap();
        assert!(!ep.is_official);
        assert!(ep.credentials_allowed);
    }

    #[test]
    fn rejects_userinfo() {
        let err = validate_endpoint(
            "https://user:pass@openrouter.ai/api/v1",
            &RemotePolicy::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("userinfo") || err.to_string().contains("credential"));
    }

    #[test]
    fn loopback_http_for_tests() {
        let policy = RemotePolicy {
            allow_loopback_http: true,
            ..Default::default()
        };
        let ep = validate_endpoint("http://127.0.0.1:9", &policy).unwrap();
        assert!(ep.credentials_allowed);
    }

    #[test]
    fn http_non_loopback_rejected() {
        assert!(validate_endpoint("http://evil.example", &RemotePolicy::default()).is_err());
    }
}
