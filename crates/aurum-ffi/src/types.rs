//! Public façade types (no raw pointers).

/// Monotonic C ABI version. Bump on any breaking C/header change.
///
/// Current surface is ABI **2** (jobs, capabilities, expanded statuses, doctor,
/// TTS jobs). Greenfield: no dual-support lag for older ABIs.
pub const AURUM_ABI_VERSION: u32 = 2;

/// Oldest ABI supported by this build (equals current version on a greenfield cut).
pub const AURUM_ABI_MIN_VERSION: u32 = 2;

/// Required PCM sample rate (Hz), mono f32.
pub const AURUM_SAMPLE_RATE: u32 = 16_000;

/// Engine construction options.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Directory for ggml model cache (host-owned path).
    pub cache_dir: String,
    /// When true, never download models (fail if missing).
    pub local_only: bool,
    /// Optional progress noise on stderr (default off for embeds).
    pub progress_logging: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cache_dir: String::new(),
            local_only: true,
            progress_logging: false,
        }
    }
}

/// Options for a single PCM transcription.
#[derive(Debug, Clone)]
pub struct TranscribeOpts {
    pub model: String,
    pub language: String,
    pub timestamps: bool,
}

impl Default for TranscribeOpts {
    fn default() -> Self {
        Self {
            model: String::new(),
            language: "auto".into(),
            timestamps: false,
        }
    }
}

/// On-device rules cleanup style (mirrors `aurum_core::cleanup::CleanupStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CleanupStyle {
    #[default]
    Raw = 0,
    Clean = 1,
    Bullets = 2,
    Professional = 3,
    Summary = 4,
}

impl CleanupStyle {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Raw),
            1 => Some(Self::Clean),
            2 => Some(Self::Bullets),
            3 => Some(Self::Professional),
            4 => Some(Self::Summary),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn to_core(self) -> aurum_core::cleanup::CleanupStyle {
        match self {
            Self::Raw => aurum_core::cleanup::CleanupStyle::Raw,
            Self::Clean => aurum_core::cleanup::CleanupStyle::Clean,
            Self::Bullets => aurum_core::cleanup::CleanupStyle::Bullets,
            Self::Professional => aurum_core::cleanup::CleanupStyle::Professional,
            Self::Summary => aurum_core::cleanup::CleanupStyle::Summary,
        }
    }
}

/// Timed segment when timestamps were requested.
#[derive(Debug, Clone)]
pub struct Segment {
    pub start_s: f64,
    pub end_s: f64,
    pub text: String,
}

/// Normalized transcription result for hosts.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub model: String,
    pub duration_secs: f64,
    pub timestamps_reliable: bool,
    pub segments: Vec<Segment>,
    pub cleanup_style: CleanupStyle,
}
