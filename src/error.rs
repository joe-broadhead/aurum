//! Error taxonomy for Aurum.
//!
//! Errors are grouped so callers (and the CLI) can present actionable messages:
//! - [`TranscriptionError::User`] — bad input, missing key, invalid model name
//! - [`TranscriptionError::Environment`] — missing ffmpeg, disk full, cache issues
//! - [`TranscriptionError::Provider`] — network, rate limit, model load failure
//! - [`TranscriptionError::Internal`] — unexpected bugs

use thiserror::Error;

/// Top-level error type used across the core library and CLI.
#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("{0}")]
    User(#[from] UserError),

    #[error("{0}")]
    Environment(#[from] EnvironmentError),

    #[error("{0}")]
    Provider(#[from] ProviderError),

    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Error)]
pub enum UserError {
    #[error("audio file not found: {path}\n  Hint: check the path and try again.")]
    FileNotFound { path: String },

    #[error("invalid audio file: {reason}\n  Hint: ensure the file is a supported audio format (mp3, m4a, wav, flac, ogg, …).")]
    InvalidAudio { reason: String },

    #[error(
        "audio is too long ({duration_secs:.1}s); maximum accepted is {max_secs:.0}s.\n  \
         Hint: split the file, or raise the limit once longer-form support lands."
    )]
    AudioTooLong { duration_secs: f64, max_secs: f64 },

    #[error(
        "decoded audio is too large ({decoded_bytes} bytes); maximum is {max_bytes} bytes.\n  \
         Hint: split the file into shorter clips."
    )]
    AudioTooLarge {
        decoded_bytes: usize,
        max_bytes: usize,
    },

    #[error("unsupported output format: {format}\n  Hint: use one of: txt, srt, json.")]
    InvalidOutputFormat { format: String },

    #[error("unknown provider: {provider}\n  Hint: use one of: local, openrouter.")]
    InvalidProvider { provider: String },

    #[error("unknown local model: {model}\n  Hint: available models: {available}")]
    InvalidModel { model: String, available: String },

    #[error(
        "OpenRouter API key is missing.\n  \
         Set OPENROUTER_API_KEY in your environment, or add api_key under [openrouter] in the config file.\n  \
         Get a key at https://openrouter.ai/keys"
    )]
    MissingApiKey,

    #[error("invalid configuration: {reason}")]
    InvalidConfig { reason: String },

    #[error("{message}")]
    Other { message: String },
}

#[derive(Debug, Error)]
pub enum EnvironmentError {
    #[error(
        "ffmpeg is required but was not found on PATH.\n  \
         Install it, then retry:\n  \
         • macOS:   brew install ffmpeg\n  \
         • Ubuntu:  sudo apt install ffmpeg\n  \
         • Windows: winget install ffmpeg\n  \
         • Or see:  https://ffmpeg.org/download.html"
    )]
    FfmpegMissing,

    #[error("ffmpeg failed: {reason}")]
    FfmpegFailed { reason: String },

    #[error("insufficient disk space while writing {path}: {reason}")]
    DiskSpace { path: String, reason: String },

    #[error("failed to access cache/config directory {path}: {reason}")]
    DirectoryAccess { path: String, reason: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{message}")]
    Other { message: String },
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("failed to load model '{model}': {reason}")]
    ModelLoad { model: String, reason: String },

    #[error("failed to download model '{model}': {reason}")]
    ModelDownload { model: String, reason: String },

    #[error("transcription failed: {reason}")]
    TranscriptionFailed { reason: String },

    #[error("network error talking to {provider}: {reason}")]
    Network { provider: String, reason: String },

    #[error(
        "rate limited by {provider}.\n  \
         Wait a moment and retry. If this persists, check your plan/quota."
    )]
    RateLimited { provider: String },

    #[error(
        "quota or billing issue with {provider}: {reason}\n  \
         Check your account balance and plan limits."
    )]
    QuotaExceeded { provider: String, reason: String },

    #[error("authentication failed for {provider}: {reason}")]
    Auth { provider: String, reason: String },

    #[error("{provider} returned an error: {reason}")]
    Remote { provider: String, reason: String },

    #[error("{message}")]
    Other { message: String },
}

impl TranscriptionError {
    /// Short category label for logging / exit-code mapping.
    pub fn category(&self) -> &'static str {
        match self {
            Self::User(_) => "user",
            Self::Environment(_) => "environment",
            Self::Provider(_) => "provider",
            Self::Internal(_) => "internal",
        }
    }

    /// Suggested process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::User(_) => 2,
            Self::Environment(_) => 3,
            Self::Provider(_) => 4,
            Self::Internal(_) => 1,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

impl From<std::io::Error> for TranscriptionError {
    fn from(value: std::io::Error) -> Self {
        Self::Environment(EnvironmentError::Io(value))
    }
}

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, TranscriptionError>;
