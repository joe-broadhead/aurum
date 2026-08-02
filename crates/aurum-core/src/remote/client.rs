//! Policy-enforcing HTTP client for remote STT and cleanup (JOE-1587, JOE-1934).
//!
//! Auth, attribution headers, and official origins come from a named
//! [`ProviderHttpPolicy`]. Timeouts, proxy, loopback, and custom-endpoint flags
//! remain on [`RemotePolicy`].

use super::policy::{normalize_request_path, OpenRouterHttpPolicy, ProviderHttpPolicy};
use crate::error::{ProviderError, Result, UserError};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Validated remote endpoint with trust classification.
#[derive(Debug, Clone)]
pub struct RemoteEndpoint {
    pub base_url: String,
    /// True when this matches an official origin of the selected provider policy.
    pub is_official: bool,
    /// True when credentials may be sent (official or explicit custom trust).
    pub credentials_allowed: bool,
    /// Provider id that validated this endpoint.
    pub provider_id: String,
}

/// Policy knobs for building the shared client (timeouts / proxy / loopback).
///
/// Provider-specific trust (origins, auth, headers, paths) lives on
/// [`ProviderHttpPolicy`], not here.
#[derive(Debug, Clone)]
pub struct RemotePolicy {
    /// Connect timeout.
    pub connect_timeout: Duration,
    /// Total request timeout.
    pub total_timeout: Duration,
    /// When false (default), system proxy is not used.
    pub use_system_proxy: bool,
    /// When true, allow custom non-official HTTPS endpoints with credentials
    /// (requires separate config opt-in; still provider-scoped).
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

/// Parse and validate a base URL under remote + provider policy.
pub fn validate_endpoint(
    raw: &str,
    remote: &RemotePolicy,
    provider: &dyn ProviderHttpPolicy,
) -> Result<RemoteEndpoint> {
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
    let is_official = provider.is_official_origin(scheme, &host);

    match scheme {
        "https" => {}
        "http" if remote.allow_loopback_http && is_loopback => {}
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

    let custom_ok = remote.allow_custom_credentialed_endpoint
        && scheme == "https"
        && provider.allows_custom_credentialed_endpoint();
    let credentials_allowed =
        is_official || (is_loopback && remote.allow_loopback_http) || custom_ok;
    if !credentials_allowed {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "credentialed remote endpoint '{trimmed}' is not an official {} origin.\n  \
                 Hint: {}",
                provider.provider_id(),
                provider.custom_endpoint_hint()
            ),
        }
        .into());
    }

    Ok(RemoteEndpoint {
        base_url: trimmed.to_string(),
        is_official,
        credentials_allowed,
        provider_id: provider.provider_id().to_string(),
    })
}

/// Hardened reqwest client shared by remote STT and cleanup.
///
/// Provider identity, origins, auth, and extra headers come from the attached
/// [`ProviderHttpPolicy`]. Transport flags remain on [`RemotePolicy`].
#[derive(Clone)]
pub struct HardenedHttpClient {
    http: Client,
    endpoint: RemoteEndpoint,
    remote_policy: RemotePolicy,
    provider: Arc<dyn ProviderHttpPolicy>,
}

impl std::fmt::Debug for HardenedHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HardenedHttpClient")
            .field("base_url", &self.endpoint.base_url)
            .field("is_official", &self.endpoint.is_official)
            .field("provider_id", &self.endpoint.provider_id)
            .finish()
    }
}

impl HardenedHttpClient {
    /// Build a client for an arbitrary named provider policy.
    pub fn build(
        base_url: Option<&str>,
        remote: RemotePolicy,
        provider: impl ProviderHttpPolicy + 'static,
    ) -> Result<Self> {
        Self::build_arc(base_url, remote, Arc::new(provider))
    }

    /// Build with a pre-wrapped policy (shared across clones).
    pub fn build_arc(
        base_url: Option<&str>,
        remote: RemotePolicy,
        provider: Arc<dyn ProviderHttpPolicy>,
    ) -> Result<Self> {
        let raw = base_url
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| provider.default_base_url());
        let endpoint = validate_endpoint(raw, &remote, provider.as_ref())?;

        let mut builder = Client::builder()
            .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(remote.connect_timeout)
            .timeout(remote.total_timeout)
            // Never follow redirects with credentials (JOE-1587).
            .redirect(reqwest::redirect::Policy::none());

        if !remote.use_system_proxy {
            builder = builder.no_proxy();
        }

        let http = builder.build().map_err(|e| ProviderError::Network {
            provider: provider.provider_id().into(),
            reason: super::status::public_network_reason(&e),
        })?;

        Ok(Self {
            http,
            endpoint,
            remote_policy: remote,
            provider,
        })
    }

    /// Convenience: OpenRouter policy (STT + cleanup default path).
    pub fn openrouter(base_url: Option<&str>, remote: RemotePolicy) -> Result<Self> {
        Self::build(base_url, remote, OpenRouterHttpPolicy)
    }

    pub fn endpoint(&self) -> &RemoteEndpoint {
        &self.endpoint
    }

    pub fn base_url(&self) -> &str {
        &self.endpoint.base_url
    }

    pub fn policy(&self) -> &RemotePolicy {
        &self.remote_policy
    }

    pub fn provider_id(&self) -> &str {
        self.provider.provider_id()
    }

    /// Build a request with policy auth + extra headers + shared request id.
    pub fn request(&self, method: Method, path: &str, api_key: &str) -> Result<RequestBuilder> {
        if !self.endpoint.credentials_allowed {
            return Err(UserError::InvalidConfig {
                reason: "credentials are not allowed for this endpoint under current policy".into(),
            }
            .into());
        }

        let Some(path) = normalize_request_path(path) else {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "remote path is empty or contains disallowed segments ({})",
                    self.provider.provider_id()
                ),
            }
            .into());
        };

        if !self.provider.allows_path(path) {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "path '{path}' is not allowed for provider {}",
                    self.provider.provider_id()
                ),
            }
            .into());
        }

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

        let mut req = self.http.request(method, url);
        req = self.provider.apply_auth(req, api_key);
        req = self.provider.apply_extra_headers(req);
        Ok(req.header(
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
/// Public reasons are **allowlisted only** (HTTP status + optional closed provider code).
/// Arbitrary remote response bodies and free-form string codes are never echoed (JOE-1914 / JOE-1920).
pub fn map_http_status(provider: &str, status: StatusCode, body: &str) -> Result<()> {
    use super::status::public_http_reason;
    let code = status.as_u16();
    // Body is only scanned for a closed local code set — never free text.
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
    use crate::remote::policy::{
        ElevenLabsHttpPolicy, OpenAiHttpPolicy, OpenRouterHttpPolicy, XaiHttpPolicy,
    };

    #[test]
    fn official_endpoint_ok() {
        let ep = validate_endpoint(
            "https://openrouter.ai/api/v1",
            &RemotePolicy::default(),
            &OpenRouterHttpPolicy,
        )
        .unwrap();
        assert!(ep.is_official);
        assert!(ep.credentials_allowed);
        assert_eq!(ep.provider_id, "openrouter");
    }

    #[test]
    fn foreign_host_rejected_by_default() {
        let err = validate_endpoint(
            "https://evil.example/api",
            &RemotePolicy::default(),
            &OpenRouterHttpPolicy,
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(
            err.to_string().contains("allow_custom_endpoint")
                || err.to_string().contains("official")
        );
    }

    #[test]
    fn openrouter_origin_not_official_for_openai_policy() {
        let err = validate_endpoint(
            "https://openrouter.ai/api/v1",
            &RemotePolicy::default(),
            &OpenAiHttpPolicy,
        )
        .unwrap_err();
        assert!(err.to_string().contains("openai") || err.to_string().contains("official"));
    }

    #[test]
    fn openai_official_ok() {
        let ep = validate_endpoint(
            "https://api.openai.com/v1",
            &RemotePolicy::default(),
            &OpenAiHttpPolicy,
        )
        .unwrap();
        assert!(ep.is_official);
    }

    #[test]
    fn elevenlabs_and_xai_official_ok() {
        let el = validate_endpoint(
            "https://api.elevenlabs.io",
            &RemotePolicy::default(),
            &ElevenLabsHttpPolicy,
        )
        .unwrap();
        assert!(el.is_official);
        let xai = validate_endpoint(
            "https://api.x.ai/v1",
            &RemotePolicy::default(),
            &XaiHttpPolicy,
        )
        .unwrap();
        assert!(xai.is_official);
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
    fn map_http_status_drops_unknown_string_provider_code() {
        let body =
            r#"{"error":{"message":"transcript: hello world secret","code":"no_endpoints"}}"#;
        let err = map_http_status("openrouter", reqwest::StatusCode::NOT_FOUND, body).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("transcript"));
        assert!(!msg.contains("hello world"));
        assert!(!msg.contains("no_endpoints"));
        assert!(msg.contains("404"));
    }

    #[test]
    fn map_http_status_drops_credential_shaped_provider_code() {
        let body =
            r#"{"error":{"message":"x","code":"sk-or-v1-TESTCANARY-JOE1920-DO-NOT-USE-001"}}"#;
        let err =
            map_http_status("openrouter", reqwest::StatusCode::UNAUTHORIZED, body).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("TESTCANARY"));
        assert!(!msg.contains("sk-or-v1"));
    }

    #[test]
    fn custom_allowed_with_opt_in() {
        let policy = RemotePolicy {
            allow_custom_credentialed_endpoint: true,
            ..Default::default()
        };
        let ep = validate_endpoint(
            "https://compatible.example/v1",
            &policy,
            &OpenRouterHttpPolicy,
        )
        .unwrap();
        assert!(!ep.is_official);
        assert!(ep.credentials_allowed);
    }

    #[test]
    fn rejects_userinfo() {
        let err = validate_endpoint(
            "https://user:pass@openrouter.ai/api/v1",
            &RemotePolicy::default(),
            &OpenRouterHttpPolicy,
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
        let ep = validate_endpoint("http://127.0.0.1:9", &policy, &OpenRouterHttpPolicy).unwrap();
        assert!(ep.credentials_allowed);
    }

    #[test]
    fn http_non_loopback_rejected() {
        assert!(validate_endpoint(
            "http://evil.example",
            &RemotePolicy::default(),
            &OpenRouterHttpPolicy
        )
        .is_err());
    }

    #[test]
    fn request_applies_openrouter_headers_only() {
        let client = HardenedHttpClient::openrouter(None, RemotePolicy::default()).unwrap();
        let req = client
            .request(Method::POST, "chat/completions", "sk-test")
            .unwrap()
            .build()
            .unwrap();
        assert!(req.headers().get("Authorization").is_some());
        assert!(
            req.headers().get("HTTP-Referer").is_some() || req.headers().get("Referer").is_some()
        );
        assert_eq!(
            req.headers()
                .get("X-OpenRouter-Title")
                .unwrap()
                .to_str()
                .unwrap(),
            "Aurum"
        );
        assert!(req.headers().get("X-Title").is_none());
        assert!(req.headers().get("X-OpenRouter-Categories").is_some());
        assert!(req.headers().get("X-Request-Id").is_some());
        assert!(req.headers().get("xi-api-key").is_none());
    }

    #[test]
    fn request_openai_has_no_openrouter_headers() {
        let client =
            HardenedHttpClient::build(None, RemotePolicy::default(), OpenAiHttpPolicy).unwrap();
        let req = client
            .request(Method::POST, "audio/transcriptions", "sk-test")
            .unwrap()
            .build()
            .unwrap();
        assert!(req.headers().get("Authorization").is_some());
        assert!(req.headers().get("HTTP-Referer").is_none());
        assert!(req.headers().get("X-Title").is_none());
        assert!(req.headers().get("X-OpenRouter-Title").is_none());
        assert!(req.headers().get("X-OpenRouter-Categories").is_none());
        assert!(req.headers().get("xi-api-key").is_none());
    }

    #[test]
    fn request_elevenlabs_uses_xi_api_key() {
        let client =
            HardenedHttpClient::build(None, RemotePolicy::default(), ElevenLabsHttpPolicy).unwrap();
        let req = client
            .request(Method::POST, "v1/text-to-speech/voice1", "el-key")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            req.headers().get("xi-api-key").unwrap().to_str().unwrap(),
            "el-key"
        );
        assert!(req.headers().get("Authorization").is_none());
        assert!(req.headers().get("HTTP-Referer").is_none());
        assert!(req.headers().get("X-Title").is_none());
        assert!(req.headers().get("X-OpenRouter-Title").is_none());
        assert!(req.headers().get("X-OpenRouter-Categories").is_none());
    }

    #[test]
    fn request_rejects_disallowed_path() {
        let client = HardenedHttpClient::openrouter(None, RemotePolicy::default()).unwrap();
        let err = client
            .request(Method::GET, "models", "sk-test")
            .unwrap_err();
        assert!(err.to_string().contains("not allowed") || err.to_string().contains("path"));
    }

    #[test]
    fn default_base_url_per_provider() {
        let or = HardenedHttpClient::openrouter(None, RemotePolicy::default()).unwrap();
        assert!(or.base_url().contains("openrouter.ai"));
        let oa =
            HardenedHttpClient::build(None, RemotePolicy::default(), OpenAiHttpPolicy).unwrap();
        assert!(oa.base_url().contains("api.openai.com"));
    }
}
