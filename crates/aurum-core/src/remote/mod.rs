//! Shared remote HTTP client and response validation (JOE-1587, JOE-1588).
//!
//! All OpenRouter STT/cleanup traffic should go through this module so
//! endpoint policy, redirects, credentials, and body caps stay consistent.

mod canary_matrix;
mod client;
mod limits;
mod status;

pub use client::{
    map_http_status, validate_endpoint, HardenedHttpClient, RemoteEndpoint, RemotePolicy,
    DEFAULT_OPENROUTER_ORIGIN,
};
pub use limits::{
    read_body_limited, validate_segments, validate_text_bounds, RemoteBodyLimits, TranscriptLimits,
    DEFAULT_CHAT_BODY_CAP, DEFAULT_CLEANUP_BODY_CAP, DEFAULT_STT_BODY_CAP,
};
pub use status::{redact_secret, redact_secret_with};
