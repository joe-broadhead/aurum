//! Shared remote HTTP client and response validation (JOE-1587, JOE-1588, JOE-1934).
//!
//! All OpenRouter STT/cleanup traffic should go through this module so
//! endpoint policy, redirects, credentials, and body caps stay consistent.
//!
//! Provider-specific origins, auth, and headers live in [`policy`] named
//! [`ProviderHttpPolicy`] implementations — not hard-coded in the client.

mod canary_matrix;
mod client;
mod limits;
mod openai_speech;
mod policy;
mod status;

pub use client::{
    map_http_status, validate_endpoint, HardenedHttpClient, RemoteEndpoint, RemotePolicy,
    DEFAULT_OPENROUTER_ORIGIN,
};
pub use limits::{
    read_body_limited, validate_segments, validate_text_bounds, RemoteBodyLimits, TranscriptLimits,
    DEFAULT_CHAT_BODY_CAP, DEFAULT_CLEANUP_BODY_CAP, DEFAULT_STT_BODY_CAP,
};
pub use openai_speech::{parse_pcm_content_type, OpenAiSpeechRequest, SpeechResponseFormat};
pub use policy::{
    normalize_request_path, AuthScheme, ElevenLabsHttpPolicy, OpenAiHttpPolicy,
    OpenRouterHttpPolicy, ProviderHttpPolicy, XaiHttpPolicy, ELEVENLABS_DEFAULT_BASE,
    ELEVENLABS_ORIGIN, OPENAI_DEFAULT_BASE, OPENAI_ORIGIN, OPENROUTER_DEFAULT_BASE,
    OPENROUTER_ORIGIN, XAI_DEFAULT_BASE, XAI_ORIGIN,
};
pub use status::{redact_secret, redact_secret_with};
