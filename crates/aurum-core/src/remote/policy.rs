//! Named provider HTTP policies for the remote transport (JOE-1934).
//!
//! Official origins, authentication scheme, and provider-specific headers live
//! here — not in the shared hardened client. A policy is compiled/reviewed code,
//! never an implicit trust decision from a free-form model id or base URL.

use reqwest::RequestBuilder;

/// How credentials are attached to outbound requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    /// `Authorization: Bearer <key>` (OpenRouter, OpenAI, xAI).
    Bearer,
    /// `xi-api-key: <key>` (ElevenLabs).
    XiApiKey,
}

/// Provider-specific HTTP trust and header contract.
///
/// Implementations must be pure reviewed code. Credentials may only be applied
/// after the transport has proven the final URL origin against this policy (or
/// an explicit custom-endpoint opt-in under [`crate::remote::RemotePolicy`]).
pub trait ProviderHttpPolicy: Send + Sync {
    /// Stable provider id (`openrouter`, `openai`, `elevenlabs`, `xai`).
    fn provider_id(&self) -> &'static str;

    /// Official HTTPS origins (scheme + host, no path), e.g. `https://openrouter.ai`.
    fn official_origins(&self) -> &'static [&'static str];

    /// Default base URL used when the caller omits an explicit endpoint.
    fn default_base_url(&self) -> &'static str;

    /// Credential attachment scheme for this provider.
    fn auth_scheme(&self) -> AuthScheme;

    /// Attach the API key using this policy's auth scheme.
    fn apply_auth(&self, req: RequestBuilder, api_key: &str) -> RequestBuilder {
        match self.auth_scheme() {
            AuthScheme::Bearer => req.header("Authorization", format!("Bearer {api_key}")),
            AuthScheme::XiApiKey => req.header("xi-api-key", api_key.to_string()),
        }
    }

    /// Provider-specific headers (never cross-applied to other providers).
    fn apply_extra_headers(&self, req: RequestBuilder) -> RequestBuilder {
        req
    }

    /// Whether `path` (relative to the validated base URL) is permitted.
    ///
    /// Paths are normalized: leading `/` stripped; `..`, backslash, and NUL rejected.
    fn allows_path(&self, path: &str) -> bool;

    /// True when `scheme`/`host` match an official origin for this policy.
    fn is_official_origin(&self, scheme: &str, host: &str) -> bool {
        if !scheme.eq_ignore_ascii_case("https") {
            return false;
        }
        let host = host.to_ascii_lowercase();
        self.official_origins().iter().any(|origin| {
            url::Url::parse(origin)
                .ok()
                .and_then(|u| {
                    let o_host = u.host_str()?.to_ascii_lowercase();
                    Some(u.scheme() == "https" && o_host == host)
                })
                .unwrap_or(false)
        })
    }

    /// Whether an explicit custom HTTPS endpoint may receive credentials when
    /// [`crate::remote::RemotePolicy::allow_custom_credentialed_endpoint`] is set.
    fn allows_custom_credentialed_endpoint(&self) -> bool {
        true
    }

    /// Operator-facing hint when a credentialed origin is rejected.
    fn custom_endpoint_hint(&self) -> String {
        let id = self.provider_id();
        let default = self.default_base_url();
        format!(
            "set {id}.allow_custom_endpoint = true only for trusted compatible APIs, \
             or use {default}"
        )
    }
}

// ── Path helpers ────────────────────────────────────────────────────────────

/// Normalize and reject dangerous path segments before allowlist checks.
pub fn normalize_request_path(path: &str) -> Option<&str> {
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return None;
    }
    if path.contains("..") || path.contains('\\') || path.contains('\0') {
        return None;
    }
    // Reject absolute URLs smuggled as "paths".
    if path.contains("://") {
        return None;
    }
    Some(path)
}

fn path_allowed(path: &str, exact: &[&str], prefixes: &[&str]) -> bool {
    let Some(path) = normalize_request_path(path) else {
        return false;
    };
    if exact.contains(&path) {
        return true;
    }
    prefixes.iter().any(|p| {
        path == *p
            || path
                .strip_prefix(p)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

// ── OpenRouter ──────────────────────────────────────────────────────────────

/// Official OpenRouter origin and default API base.
pub const OPENROUTER_ORIGIN: &str = "https://openrouter.ai";
pub const OPENROUTER_DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";

/// OpenRouter [app attribution](https://openrouter.ai/docs/app-attribution) for Aurum.
///
/// `HTTP-Referer` is the primary app id in OpenRouter rankings; keep this URL stable.
pub const OPENROUTER_APP_REFERER: &str = "https://github.com/joe-broadhead/aurum";
/// Display name on openrouter.ai rankings / analytics (`X-OpenRouter-Title`; `X-Title` kept for compat).
pub const OPENROUTER_APP_TITLE: &str = "Aurum";
/// Marketplace categories (max 2 per request; lowercase hyphen-separated).
pub const OPENROUTER_APP_CATEGORIES: &str = "audio-gen,cli-agent";

/// OpenRouter HTTP policy (Bearer + attribution headers).
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenRouterHttpPolicy;

impl ProviderHttpPolicy for OpenRouterHttpPolicy {
    fn provider_id(&self) -> &'static str {
        "openrouter"
    }

    fn official_origins(&self) -> &'static [&'static str] {
        &[OPENROUTER_ORIGIN]
    }

    fn default_base_url(&self) -> &'static str {
        OPENROUTER_DEFAULT_BASE
    }

    fn auth_scheme(&self) -> AuthScheme {
        AuthScheme::Bearer
    }

    fn apply_extra_headers(&self, req: RequestBuilder) -> RequestBuilder {
        // https://openrouter.ai/docs/app-attribution
        // HTTP-Referer is required for rankings; title alone does not create an app page.
        req.header("HTTP-Referer", OPENROUTER_APP_REFERER)
            .header("X-OpenRouter-Title", OPENROUTER_APP_TITLE)
            // Backwards-compatible alias still accepted by OpenRouter.
            .header("X-Title", OPENROUTER_APP_TITLE)
            .header("X-OpenRouter-Categories", OPENROUTER_APP_CATEGORIES)
    }

    fn allows_path(&self, path: &str) -> bool {
        // STT + TTS speech + cleanup surfaces currently used by Aurum (JOE-1939).
        path_allowed(
            path,
            &["chat/completions", "audio/transcriptions", "audio/speech"],
            &[],
        )
    }

    fn custom_endpoint_hint(&self) -> String {
        format!(
            "set openrouter.allow_custom_endpoint = true only for trusted compatible APIs, \
             or use {OPENROUTER_ORIGIN}."
        )
    }
}

// ── OpenAI ──────────────────────────────────────────────────────────────────

pub const OPENAI_ORIGIN: &str = "https://api.openai.com";
pub const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";

/// OpenAI first-party HTTP policy (Bearer; no OpenRouter attribution headers).
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiHttpPolicy;

impl ProviderHttpPolicy for OpenAiHttpPolicy {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    fn official_origins(&self) -> &'static [&'static str] {
        &[OPENAI_ORIGIN]
    }

    fn default_base_url(&self) -> &'static str {
        OPENAI_DEFAULT_BASE
    }

    fn auth_scheme(&self) -> AuthScheme {
        AuthScheme::Bearer
    }

    fn allows_path(&self, path: &str) -> bool {
        path_allowed(
            path,
            &[
                "chat/completions",
                "audio/transcriptions",
                "audio/translations",
                "audio/speech",
            ],
            &[],
        )
    }
}

// ── ElevenLabs ──────────────────────────────────────────────────────────────

pub const ELEVENLABS_ORIGIN: &str = "https://api.elevenlabs.io";
pub const ELEVENLABS_DEFAULT_BASE: &str = "https://api.elevenlabs.io";

/// ElevenLabs HTTP policy (`xi-api-key`; no Bearer / OpenRouter headers).
#[derive(Debug, Default, Clone, Copy)]
pub struct ElevenLabsHttpPolicy;

impl ProviderHttpPolicy for ElevenLabsHttpPolicy {
    fn provider_id(&self) -> &'static str {
        "elevenlabs"
    }

    fn official_origins(&self) -> &'static [&'static str] {
        &[ELEVENLABS_ORIGIN]
    }

    fn default_base_url(&self) -> &'static str {
        ELEVENLABS_DEFAULT_BASE
    }

    fn auth_scheme(&self) -> AuthScheme {
        AuthScheme::XiApiKey
    }

    fn allows_path(&self, path: &str) -> bool {
        // Prefix allowlist: voice-id suffixes under TTS/STT routes.
        path_allowed(
            path,
            &["v1/speech-to-text"],
            &["v1/text-to-speech", "v1/speech-to-speech"],
        )
    }
}

// ── xAI ─────────────────────────────────────────────────────────────────────

pub const XAI_ORIGIN: &str = "https://api.x.ai";
pub const XAI_DEFAULT_BASE: &str = "https://api.x.ai/v1";

/// xAI HTTP policy (Bearer; no OpenRouter attribution headers).
#[derive(Debug, Default, Clone, Copy)]
pub struct XaiHttpPolicy;

impl ProviderHttpPolicy for XaiHttpPolicy {
    fn provider_id(&self) -> &'static str {
        "xai"
    }

    fn official_origins(&self) -> &'static [&'static str] {
        &[XAI_ORIGIN]
    }

    fn default_base_url(&self) -> &'static str {
        XAI_DEFAULT_BASE
    }

    fn auth_scheme(&self) -> AuthScheme {
        AuthScheme::Bearer
    }

    fn allows_path(&self, path: &str) -> bool {
        // Official Voice REST (JOE-1976): POST /v1/stt and POST /v1/tts only.
        // OpenAI-shaped /audio/* and realtime/WebSocket remain denied.
        path_allowed(path, &["stt", "tts"], &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;

    #[test]
    fn openrouter_extra_headers_present() {
        let p = OpenRouterHttpPolicy;
        let req = p
            .apply_extra_headers(Client::new().get("https://openrouter.ai/api/v1/x"))
            .build()
            .unwrap();
        let headers = req.headers();
        let names: Vec<_> = headers
            .keys()
            .map(|k| k.as_str().to_ascii_lowercase())
            .collect();
        assert!(names.iter().any(|n| n == "http-referer" || n == "referer"));
        assert!(names.iter().any(|n| n == "x-openrouter-title"));
        assert!(names.iter().any(|n| n == "x-title"));
        assert!(names.iter().any(|n| n == "x-openrouter-categories"));

        let referer = headers
            .get("HTTP-Referer")
            .or_else(|| headers.get("Referer"))
            .and_then(|v| v.to_str().ok())
            .unwrap();
        assert_eq!(referer, OPENROUTER_APP_REFERER);
        assert_eq!(
            headers.get("X-OpenRouter-Title").unwrap().to_str().unwrap(),
            OPENROUTER_APP_TITLE
        );
        assert_eq!(
            headers.get("X-Title").unwrap().to_str().unwrap(),
            OPENROUTER_APP_TITLE
        );
        assert_eq!(
            headers
                .get("X-OpenRouter-Categories")
                .unwrap()
                .to_str()
                .unwrap(),
            OPENROUTER_APP_CATEGORIES
        );
    }

    #[test]
    fn openrouter_headers_do_not_cross_to_openai() {
        let p = OpenAiHttpPolicy;
        let req = p
            .apply_extra_headers(Client::new().get("https://api.openai.com/v1/x"))
            .build()
            .unwrap();
        for name in req.headers().keys() {
            let n = name.as_str().to_ascii_lowercase();
            assert_ne!(n, "http-referer");
            assert_ne!(n, "x-title");
            assert_ne!(n, "x-openrouter-title");
            assert_ne!(n, "x-openrouter-categories");
            assert_ne!(n, "xi-api-key");
        }
    }

    #[test]
    fn openrouter_headers_do_not_cross_to_elevenlabs_or_xai() {
        for req in [
            ElevenLabsHttpPolicy
                .apply_extra_headers(Client::new().get("https://api.elevenlabs.io/x"))
                .build()
                .unwrap(),
            XaiHttpPolicy
                .apply_extra_headers(Client::new().get("https://api.x.ai/v1/x"))
                .build()
                .unwrap(),
        ] {
            for name in req.headers().keys() {
                let n = name.as_str().to_ascii_lowercase();
                assert_ne!(n, "http-referer");
                assert_ne!(n, "x-title");
                assert_ne!(n, "x-openrouter-title");
                assert_ne!(n, "x-openrouter-categories");
            }
        }
    }

    #[test]
    fn auth_schemes_do_not_cross() {
        let key = "test-key-canary";
        let bearer = OpenAiHttpPolicy
            .apply_auth(Client::new().get("https://api.openai.com/v1/x"), key)
            .build()
            .unwrap();
        assert!(bearer.headers().get("Authorization").is_some());
        assert!(bearer.headers().get("xi-api-key").is_none());

        let xi = ElevenLabsHttpPolicy
            .apply_auth(Client::new().get("https://api.elevenlabs.io/x"), key)
            .build()
            .unwrap();
        assert!(xi.headers().get("xi-api-key").is_some());
        assert!(xi.headers().get("Authorization").is_none());
        assert!(!xi
            .headers()
            .get("xi-api-key")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Bearer"));
    }

    #[test]
    fn official_origins_are_provider_scoped() {
        assert!(OpenRouterHttpPolicy.is_official_origin("https", "openrouter.ai"));
        assert!(!OpenRouterHttpPolicy.is_official_origin("https", "api.openai.com"));
        assert!(!OpenRouterHttpPolicy.is_official_origin("http", "openrouter.ai"));

        assert!(OpenAiHttpPolicy.is_official_origin("https", "api.openai.com"));
        assert!(!OpenAiHttpPolicy.is_official_origin("https", "openrouter.ai"));

        assert!(ElevenLabsHttpPolicy.is_official_origin("https", "api.elevenlabs.io"));
        assert!(XaiHttpPolicy.is_official_origin("https", "api.x.ai"));
        assert!(!XaiHttpPolicy.is_official_origin("https", "api.openai.com"));
    }

    #[test]
    fn path_allowlists_reject_traversal_and_foreign() {
        assert!(OpenRouterHttpPolicy.allows_path("chat/completions"));
        assert!(OpenRouterHttpPolicy.allows_path("/audio/transcriptions"));
        assert!(OpenRouterHttpPolicy.allows_path("audio/speech"));
        assert!(!OpenRouterHttpPolicy.allows_path("../chat/completions"));
        assert!(!OpenRouterHttpPolicy.allows_path("https://evil.example/x"));
        assert!(!OpenRouterHttpPolicy.allows_path("models"));

        assert!(OpenAiHttpPolicy.allows_path("audio/speech"));
        assert!(!OpenAiHttpPolicy.allows_path("v1/text-to-speech/abc"));

        assert!(ElevenLabsHttpPolicy.allows_path("v1/text-to-speech/voiceid"));
        assert!(!ElevenLabsHttpPolicy.allows_path("chat/completions"));

        assert!(XaiHttpPolicy.allows_path("stt"));
        assert!(XaiHttpPolicy.allows_path("tts"));
        assert!(!XaiHttpPolicy.allows_path("audio/transcriptions"));
        assert!(!XaiHttpPolicy.allows_path("audio/speech"));
        assert!(!XaiHttpPolicy.allows_path("chat/completions"));
        assert!(!XaiHttpPolicy.allows_path("realtime"));
    }

    #[test]
    fn provider_ids_stable() {
        assert_eq!(OpenRouterHttpPolicy.provider_id(), "openrouter");
        assert_eq!(OpenAiHttpPolicy.provider_id(), "openai");
        assert_eq!(ElevenLabsHttpPolicy.provider_id(), "elevenlabs");
        assert_eq!(XaiHttpPolicy.provider_id(), "xai");
    }
}
