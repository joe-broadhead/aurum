//! FFI error taxonomy (stable status codes + message).

use aurum_core::error::{EnvironmentError, ProviderError, TranscriptionError, UserError};
use std::fmt;

/// Stable C-compatible status codes (`AurumStatus` in `aurum.h`).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiStatus {
    Ok = 0,
    InvalidArg = 1,
    State = 2,
    ModelNotReady = 3,
    ModelDownload = 4,
    Inference = 5,
    Cancelled = 6,
    Audio = 7,
    Internal = 8,
    Unsupported = 9,
    NoMemory = 10,
    /// Shutdown drain timed out; active work remains (JOE-1594).
    Busy = 11,
    /// Operation deadline exceeded (JOE-1595).
    Deadline = 12,
    /// Resource governor rejected admission (JOE-1596).
    Overload = 13,
    /// Artifact digest/size integrity failure (JOE-1624).
    ArtifactIntegrity = 14,
    /// Remote/network failure.
    Network = 15,
    /// Authentication failure.
    Auth = 16,
    /// Quota exhausted.
    Quota = 17,
    /// Rate limited.
    RateLimit = 18,
    /// Filesystem / path access failure.
    Filesystem = 19,
    /// Engine or process is shutting down / stopped.
    Shutdown = 20,
}

impl FfiStatus {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Façade error with stable code + human message.
#[derive(Debug, Clone)]
pub struct FfiError {
    pub status: FfiStatus,
    pub message: String,
}

impl FfiError {
    pub fn new(status: FfiStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn invalid_arg(msg: impl Into<String>) -> Self {
        Self::new(FfiStatus::InvalidArg, msg)
    }

    pub fn state(msg: impl Into<String>) -> Self {
        Self::new(FfiStatus::State, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(FfiStatus::Internal, msg)
    }
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FfiError {}

impl From<TranscriptionError> for FfiError {
    fn from(err: TranscriptionError) -> Self {
        let message = err.to_string();
        let status = match &err {
            TranscriptionError::User(u) => match u {
                UserError::ModelNotCached { .. } => FfiStatus::ModelNotReady,
                UserError::UnsupportedSampleRate { .. }
                | UserError::AudioTooLong { .. }
                | UserError::AudioTooLarge { .. }
                | UserError::InvalidAudio { .. } => FfiStatus::Audio,
                UserError::InvalidModel { .. }
                | UserError::InvalidConfig { .. }
                | UserError::FileNotFound { .. }
                | UserError::InvalidOutputFormat { .. }
                | UserError::InvalidProvider { .. }
                | UserError::MissingApiKey
                | UserError::MissingProviderCredential { .. }
                | UserError::UnsupportedCapability { .. }
                | UserError::Other { .. } => FfiStatus::InvalidArg,
            },
            TranscriptionError::Environment(e) => match e {
                EnvironmentError::DiskSpace { .. } => FfiStatus::NoMemory,
                EnvironmentError::DirectoryAccess { .. } | EnvironmentError::Io(_) => {
                    FfiStatus::Filesystem
                }
                EnvironmentError::FfmpegMissing
                | EnvironmentError::FfmpegFailed { .. }
                | EnvironmentError::Other { .. } => FfiStatus::Audio,
            },
            TranscriptionError::Provider(p) => match p {
                ProviderError::Cancelled => FfiStatus::Cancelled,
                ProviderError::DeadlineExceeded => FfiStatus::Deadline,
                ProviderError::Overload { .. } => FfiStatus::Overload,
                ProviderError::ModelDownload { .. } => FfiStatus::ModelDownload,
                ProviderError::Network { .. } => FfiStatus::Network,
                ProviderError::RateLimited { .. } => FfiStatus::RateLimit,
                ProviderError::QuotaExceeded { .. } => FfiStatus::Quota,
                ProviderError::Auth { .. } => FfiStatus::Auth,
                ProviderError::ModelLoad { .. }
                | ProviderError::TranscriptionFailed { .. }
                | ProviderError::Remote { .. }
                | ProviderError::ResponseTooLarge { .. }
                | ProviderError::InvalidProviderPayload { .. }
                | ProviderError::LimitExceeded { .. }
                | ProviderError::Other { .. } => FfiStatus::Inference,
            },
            TranscriptionError::Internal(_) => FfiStatus::Internal,
        };
        Self { status, message }
    }
}
