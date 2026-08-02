//! Configuration loading for Aurum (JOE-1935 provider profiles).
//!
//! # Precedence (highest wins)
//!
//! 1. CLI flags (via [`Config::apply_cli`] / [`ValidatedConfig::apply_cli`])
//! 2. Environment variables for provider secrets and a few overrides
//! 3. Config file
//! 4. Built-in defaults
//!
//! # Schema (canonical)
//!
//! ```toml
//! [stt]
//! provider = "local"
//! model = "base"
//! language = "auto"
//!
//! [tts]
//! provider = "local"
//! model = "kitten-nano-int8"
//! voice = "Luna"
//! language = "en"
//! speaking_rate = 1.0
//!
//! [providers.openrouter]
//! # api_key from OPENROUTER_API_KEY preferred
//! stt_mode = "auto"
//!
//! [providers.openai]
//! # api_key from OPENAI_API_KEY
//!
//! [providers.elevenlabs]
//! # api_key from ELEVENLABS_API_KEY
//!
//! [providers.xai]
//! # api_key from XAI_API_KEY
//! ```
//!
//! Only the canonical sections above are accepted. There is no dual-path
//! migration for older TOML layouts.
//!
//! A provider is **never** inferred merely because its API key is present.

use crate::error::{Result, UserError};
use crate::provider_platform::ProviderId;
use crate::secret::SecretString;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Built-in defaults used when nothing else is set.
pub const DEFAULT_PROVIDER: &str = "local";
pub const DEFAULT_LOCAL_MODEL: &str = "base";
pub const DEFAULT_OPENROUTER_MODEL: &str = "google/gemini-2.5-flash";
pub const DEFAULT_LANGUAGE: &str = "auto";
pub const DEFAULT_OUTPUT: &str = "txt";
pub const DEFAULT_CLEANUP: &str = "raw";
pub const DEFAULT_CLEANUP_PROVIDER: &str = "rules";
pub const DEFAULT_TTS_PROVIDER: &str = "local";
pub const DEFAULT_TTS_LANGUAGE: &str = "en";
pub const DEFAULT_TTS_MAX_CHARS: usize = 5_000;
pub const DEFAULT_TTS_TIMEOUT_MS: u64 = 120_000;
pub const DEFAULT_TTS_SPEAKING_RATE: f32 = 1.0;
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// On-disk configuration file schema (canonical sections only).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// STT direction (`[stt]`).
    #[serde(default)]
    pub stt: Option<SttSection>,
    #[serde(default)]
    pub cleanup: CleanupSection,
    #[serde(default)]
    pub tts: TtsSection,
    /// Named provider credentials and vendor options.
    #[serde(default)]
    pub providers: ProvidersFileSection,
}

/// Canonical STT direction (`[stt]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttSection {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_local_model")]
    pub model: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_output")]
    pub output: String,
}

impl Default for SttSection {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_local_model(),
            language: default_language(),
            output: default_output(),
        }
    }
}

/// `[providers.openrouter]` credentials and vendor options.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRouterSection {
    /// Prefer `OPENROUTER_API_KEY` env var over this field.
    ///
    /// Stored as [`SecretString`]: Debug/Display/Serialize never emit plaintext
    /// (JOE-1914). Deserialize still loads the value from TOML.
    pub api_key: Option<SecretString>,
    /// Default remote model when provider is openrouter.
    pub model: Option<String>,
    /// Optional custom base URL (for testing / proxies).
    pub base_url: Option<String>,
    /// Allow credentialed non-OpenRouter HTTPS endpoints (JOE-1587). Default false.
    #[serde(default)]
    pub allow_custom_endpoint: bool,
    /// STT path mode: `auto` | `chat` | `transcriptions` (JOE-1586).
    #[serde(default = "default_stt_mode")]
    pub stt_mode: String,
    /// Use system HTTP(S)_PROXY (privacy implications). Default false.
    #[serde(default)]
    pub use_system_proxy: bool,
}

impl std::fmt::Debug for OpenRouterSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterSection")
            .field("api_key", &self.api_key)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("allow_custom_endpoint", &self.allow_custom_endpoint)
            .field("stt_mode", &self.stt_mode)
            .field("use_system_proxy", &self.use_system_proxy)
            .finish()
    }
}

fn default_stt_mode() -> String {
    "auto".into()
}

/// Shared credential + optional base URL for named remote providers.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCredentialSection {
    pub api_key: Option<SecretString>,
    pub base_url: Option<String>,
}

impl std::fmt::Debug for ProviderCredentialSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderCredentialSection")
            .field("api_key", &self.api_key)
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// `[providers.*]` file section — only known provider ids (deny unknown).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvidersFileSection {
    #[serde(default)]
    pub openrouter: Option<OpenRouterSection>,
    #[serde(default)]
    pub openai: Option<ProviderCredentialSection>,
    #[serde(default)]
    pub elevenlabs: Option<ProviderCredentialSection>,
    #[serde(default)]
    pub xai: Option<ProviderCredentialSection>,
}

/// Runtime provider credential block (no ever-growing flat secrets list).
#[derive(Clone, Default)]
pub struct ProviderCredentialConfig {
    pub api_key: Option<SecretString>,
    pub base_url: Option<String>,
}

impl std::fmt::Debug for ProviderCredentialConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderCredentialConfig")
            .field("api_key", &self.api_key)
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// Typed named provider configs on the runtime [`Config`].
#[derive(Clone, Default)]
pub struct ProvidersConfig {
    pub openai: ProviderCredentialConfig,
    pub elevenlabs: ProviderCredentialConfig,
    pub xai: ProviderCredentialConfig,
}

impl std::fmt::Debug for ProvidersConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProvidersConfig")
            .field("openai", &self.openai)
            .field("elevenlabs", &self.elevenlabs)
            .field("xai", &self.xai)
            .finish()
    }
}

/// Post-ASR cleanup defaults (`[cleanup]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupSection {
    /// `raw` | `clean` | `bullets` | `professional` | `summary`
    #[serde(default = "default_cleanup")]
    pub style: String,
    /// `rules` (on-device) | `openrouter`
    #[serde(default = "default_cleanup_provider")]
    pub provider: String,
    /// Optional model id when provider is openrouter.
    pub openrouter_model: Option<String>,
}

impl Default for CleanupSection {
    fn default() -> Self {
        Self {
            style: default_cleanup(),
            provider: default_cleanup_provider(),
            openrouter_model: None,
        }
    }
}

/// TTS direction defaults (`[tts]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsSection {
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    #[serde(default = "default_tts_model")]
    pub model: String,
    #[serde(default = "default_tts_voice")]
    pub voice: String,
    #[serde(default = "default_tts_language")]
    pub language: String,
    /// Playback / synthesis rate multiplier (1.0 = normal).
    #[serde(default = "default_tts_speaking_rate")]
    pub speaking_rate: f32,
    #[serde(default = "default_tts_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_tts_timeout_ms")]
    pub timeout_ms: u64,
    /// Optional default local model-pack directory (JOE-1619). CLI `--pack-dir`
    /// overrides this. Never shadows built-in catalogue cache identity.
    #[serde(default)]
    pub pack_dir: Option<String>,
    /// Allow `local_unverified` packs when `pack_dir` / CLI pack is used.
    #[serde(default)]
    pub allow_unverified: bool,
    /// Custom catalogue entries for supported adapters (JOE-1620).
    #[serde(default)]
    pub custom_models: Vec<CustomTtsModelConfig>,
}

/// Config file form of `[[tts.custom_models]]` (JOE-1620).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomTtsModelConfig {
    pub id: String,
    pub adapter: String,
    #[serde(default)]
    pub pack_dir: Option<String>,
    pub trust: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Default for TtsSection {
    fn default() -> Self {
        Self {
            provider: default_tts_provider(),
            model: default_tts_model(),
            voice: default_tts_voice(),
            language: default_tts_language(),
            speaking_rate: default_tts_speaking_rate(),
            max_chars: default_tts_max_chars(),
            timeout_ms: default_tts_timeout_ms(),
            pack_dir: None,
            allow_unverified: false,
            custom_models: Vec::new(),
        }
    }
}

fn default_provider() -> String {
    DEFAULT_PROVIDER.to_string()
}
fn default_local_model() -> String {
    DEFAULT_LOCAL_MODEL.to_string()
}
fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}
fn default_output() -> String {
    DEFAULT_OUTPUT.to_string()
}
fn default_cleanup() -> String {
    DEFAULT_CLEANUP.to_string()
}
fn default_cleanup_provider() -> String {
    DEFAULT_CLEANUP_PROVIDER.to_string()
}
fn default_tts_provider() -> String {
    DEFAULT_TTS_PROVIDER.to_string()
}
fn default_tts_model() -> String {
    #[cfg(feature = "tts")]
    {
        crate::tts::DEFAULT_TTS_MODEL.to_string()
    }
    #[cfg(not(feature = "tts"))]
    {
        "kitten-nano-int8".to_string()
    }
}
fn default_tts_voice() -> String {
    #[cfg(feature = "tts")]
    {
        crate::tts::DEFAULT_TTS_VOICE.to_string()
    }
    #[cfg(not(feature = "tts"))]
    {
        "Luna".to_string()
    }
}
fn default_tts_language() -> String {
    DEFAULT_TTS_LANGUAGE.to_string()
}
fn default_tts_max_chars() -> usize {
    DEFAULT_TTS_MAX_CHARS
}
fn default_tts_timeout_ms() -> u64 {
    DEFAULT_TTS_TIMEOUT_MS
}
fn default_tts_speaking_rate() -> f32 {
    DEFAULT_TTS_SPEAKING_RATE
}

/// Fully-resolved runtime configuration after merging all sources.
///
/// STT direction uses `provider` / `model` / `language` / `output`. TTS uses
/// `tts_*`. OpenRouter options are mirrored onto the `openrouter_*` fields from
/// `[providers.openrouter]`. Other vendors live under [`Config::providers`] so
/// the flat surface does not grow per vendor.
#[derive(Clone)]
pub struct Config {
    /// STT provider id (`local`, `openrouter`, …). Never inferred from key presence.
    pub provider: String,
    pub model: Option<String>,
    pub language: String,
    pub output: String,
    pub output_file: Option<PathBuf>,
    pub timestamps: bool,
    pub verbose: bool,
    /// OpenRouter API key — redacted via [`SecretString`] (JOE-1779).
    pub openrouter_api_key: Option<SecretString>,
    pub openrouter_base_url: String,
    pub openrouter_default_model: String,
    /// Allow custom credentialed endpoints (JOE-1587).
    pub openrouter_allow_custom_endpoint: bool,
    /// `auto` | `chat` | `transcriptions` (JOE-1586).
    pub openrouter_stt_mode: String,
    pub openrouter_use_system_proxy: bool,
    /// Named non-OpenRouter provider credentials (JOE-1935).
    pub providers: ProvidersConfig,
    /// Cleanup style name (`raw`, `clean`, …).
    pub cleanup_style: String,
    /// Cleanup backend name (`rules`, `openrouter`).
    pub cleanup_provider: String,
    /// Optional dedicated model for OpenRouter cleanup.
    pub cleanup_openrouter_model: Option<String>,
    /// TTS provider name (default `local`).
    pub tts_provider: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub tts_language: String,
    pub tts_speaking_rate: f32,
    pub tts_max_chars: usize,
    pub tts_timeout_ms: u64,
    /// Optional default pack directory for local override (JOE-1619).
    pub tts_pack_dir: Option<PathBuf>,
    pub tts_allow_unverified: bool,
    /// Validated custom TTS catalogue entries (empty when packs not present yet).
    pub tts_custom_models: Vec<CustomTtsModelConfig>,
    /// When true, remote STT/TTS providers are rejected at validation (JOE-1935).
    pub local_only: bool,
    pub config_path: Option<PathBuf>,
    pub cache_dir: PathBuf,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("language", &self.language)
            .field("output", &self.output)
            .field("output_file", &self.output_file)
            .field("timestamps", &self.timestamps)
            .field("verbose", &self.verbose)
            .field("openrouter_api_key", &self.openrouter_api_key)
            .field("openrouter_base_url", &self.openrouter_base_url)
            .field("openrouter_default_model", &self.openrouter_default_model)
            .field(
                "openrouter_allow_custom_endpoint",
                &self.openrouter_allow_custom_endpoint,
            )
            .field("openrouter_stt_mode", &self.openrouter_stt_mode)
            .field(
                "openrouter_use_system_proxy",
                &self.openrouter_use_system_proxy,
            )
            .field("providers", &self.providers)
            .field("cleanup_style", &self.cleanup_style)
            .field("cleanup_provider", &self.cleanup_provider)
            .field("cleanup_openrouter_model", &self.cleanup_openrouter_model)
            .field("tts_provider", &self.tts_provider)
            .field("tts_model", &self.tts_model)
            .field("tts_voice", &self.tts_voice)
            .field("tts_language", &self.tts_language)
            .field("tts_speaking_rate", &self.tts_speaking_rate)
            .field("tts_max_chars", &self.tts_max_chars)
            .field("tts_timeout_ms", &self.tts_timeout_ms)
            .field("tts_pack_dir", &self.tts_pack_dir)
            .field("tts_allow_unverified", &self.tts_allow_unverified)
            .field("tts_custom_models", &self.tts_custom_models)
            .field("local_only", &self.local_only)
            .field("config_path", &self.config_path)
            .field("cache_dir", &self.cache_dir)
            .finish()
    }
}

/// Source of an effective config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValueSource {
    Default,
    File,
    Environment,
    Cli,
}

/// Attribution for key fields (diagnostics only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSourceMap {
    pub provider: ConfigValueSource,
    pub openrouter_api_key: ConfigValueSource,
    pub openrouter_base_url: ConfigValueSource,
    pub openai_api_key: ConfigValueSource,
    pub elevenlabs_api_key: ConfigValueSource,
    pub xai_api_key: ConfigValueSource,
    pub tts_model: ConfigValueSource,
}

impl ConfigSourceMap {
    fn default_attribution(cfg: &Config) -> Self {
        let key_src =
            env_or_file_or_default("OPENROUTER_API_KEY", cfg.openrouter_api_key.is_some());
        let base_src = if env_nonempty("OPENROUTER_BASE_URL") {
            ConfigValueSource::Environment
        } else {
            ConfigValueSource::File
        };
        let tts_src = if env_nonempty("AURUM_TTS_MODEL") {
            ConfigValueSource::Environment
        } else {
            ConfigValueSource::File
        };
        Self {
            provider: if cfg.config_path.is_some() {
                ConfigValueSource::File
            } else {
                ConfigValueSource::Default
            },
            openrouter_api_key: key_src,
            openrouter_base_url: base_src,
            openai_api_key: env_or_file_or_default(
                "OPENAI_API_KEY",
                cfg.providers.openai.api_key.is_some(),
            ),
            elevenlabs_api_key: env_or_file_or_default(
                "ELEVENLABS_API_KEY",
                cfg.providers.elevenlabs.api_key.is_some(),
            ),
            xai_api_key: env_or_file_or_default("XAI_API_KEY", cfg.providers.xai.api_key.is_some()),
            tts_model: tts_src,
        }
    }
}

fn env_nonempty(name: &str) -> bool {
    std::env::var(name).ok().filter(|s| !s.is_empty()).is_some()
}

fn env_or_file_or_default(env_name: &str, file_present: bool) -> ConfigValueSource {
    if env_nonempty(env_name) {
        ConfigValueSource::Environment
    } else if file_present {
        ConfigValueSource::File
    } else {
        ConfigValueSource::Default
    }
}

/// Redacted presence metadata for one provider credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSecretDiagnostic {
    /// `Some("***")` when a key is present; `None` when absent. Never plaintext.
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub api_key_source: ConfigValueSource,
}

/// Redacted JSON-serializable effective config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveConfigDiagnostic {
    pub provider: String,
    pub model: Option<String>,
    pub language: String,
    pub output: String,
    pub timestamps: bool,
    pub openrouter_api_key: Option<String>,
    pub openrouter_base_url: String,
    pub openrouter_default_model: String,
    pub openrouter_stt_mode: String,
    pub openrouter_allow_custom_endpoint: bool,
    /// Redacted provider secret presence (JOE-1935).
    pub providers: ProvidersDiagnostic,
    pub cleanup_style: String,
    pub cleanup_provider: String,
    pub tts_provider: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub tts_language: String,
    pub tts_speaking_rate: f32,
    pub tts_max_chars: usize,
    pub tts_timeout_ms: u64,
    pub tts_pack_dir: Option<String>,
    pub tts_allow_unverified: bool,
    pub tts_custom_model_ids: Vec<String>,
    pub local_only: bool,
    pub config_path: Option<String>,
    pub cache_dir: String,
    pub sources: ConfigSourceMap,
}

/// Redacted view of named provider credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersDiagnostic {
    pub openrouter: ProviderSecretDiagnostic,
    pub openai: ProviderSecretDiagnostic,
    pub elevenlabs: ProviderSecretDiagnostic,
    pub xai: ProviderSecretDiagnostic,
}

impl Config {
    /// Resolve the platform-appropriate config file path.
    pub fn default_config_path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "aurum").map(|d| d.config_dir().join("config.toml"))
    }

    /// Resolve the platform-appropriate cache directory (models live under `models/`).
    pub fn default_cache_dir() -> Result<PathBuf> {
        if let Some(dirs) = ProjectDirs::from("", "", "aurum") {
            return Ok(dirs.cache_dir().to_path_buf());
        }
        let home = dirs_home()?;
        Ok(home.join(".cache").join("aurum"))
    }

    /// Load config file from the default location (if present) and merge with env vars.
    pub fn load() -> Result<Self> {
        let path = Self::default_config_path();
        let file = match &path {
            Some(p) if p.exists() => Some(load_config_file(p)?),
            _ => None,
        };
        let cfg = Self::from_parts(file, path)?;
        cfg.validate_tts_custom_models()?;
        Ok(cfg)
    }

    /// Load from an explicit config file path (used in tests / CLI `--config`).
    pub fn load_from(path: &Path) -> Result<Self> {
        let file = if path.exists() {
            Some(load_config_file(path)?)
        } else {
            None
        };
        let cfg = Self::from_parts(file, Some(path.to_path_buf()))?;
        cfg.validate_tts_custom_models()?;
        Ok(cfg)
    }

    /// Load from an explicit path; **error** if the file is missing (JOE-1608).
    pub fn load_from_required(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "config file not found: {}\n  Hint: create it or omit --config to use defaults",
                    path.display()
                ),
            }
            .into());
        }
        let file = load_config_file(path)?;
        let cfg = Self::from_parts(Some(file), Some(path.to_path_buf()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Provider-scoped secret for build-context construction (JOE-1935).
    pub fn provider_secret(&self, id: &ProviderId) -> Option<SecretString> {
        match id.as_str() {
            "openrouter" => self.openrouter_api_key.clone(),
            "openai" => self.providers.openai.api_key.clone(),
            "elevenlabs" => self.providers.elevenlabs.api_key.clone(),
            "xai" => self.providers.xai.api_key.clone(),
            _ => None,
        }
    }

    /// Validate provider/model/style/limit cross-fields (JOE-1608 / JOE-1935).
    pub fn validate(&self) -> Result<()> {
        validate_stt_provider(&self.provider)?;
        validate_tts_provider(&self.tts_provider)?;
        let _ = crate::output::OutputFormat::parse(&self.output)?;
        let _ = crate::cleanup::CleanupStyle::parse(&self.cleanup_style)?;
        let _ = crate::cleanup::CleanupProviderKind::parse(&self.cleanup_provider)?;
        let _ = crate::providers::OpenRouterSttMode::parse(&self.openrouter_stt_mode)?;

        if self.tts_max_chars == 0 {
            return Err(UserError::InvalidConfig {
                reason: "tts.max_chars must be >= 1".into(),
            }
            .into());
        }
        if self.tts_timeout_ms == 0 {
            return Err(UserError::InvalidConfig {
                reason: "tts.timeout_ms must be >= 1".into(),
            }
            .into());
        }
        if self.tts_max_chars > 500_000 {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "tts.max_chars {} exceeds safe ceiling 500000",
                    self.tts_max_chars
                ),
            }
            .into());
        }
        if !self.tts_speaking_rate.is_finite()
            || self.tts_speaking_rate <= 0.0
            || self.tts_speaking_rate > 4.0
        {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "tts.speaking_rate must be finite and in (0, 4] (got {})",
                    self.tts_speaking_rate
                ),
            }
            .into());
        }
        if !self.openrouter_base_url.starts_with("https://")
            && !self.openrouter_base_url.starts_with("http://localhost")
            && !self.openrouter_base_url.contains("127.0.0.1")
            && self.openrouter_base_url.starts_with("http://")
        {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "openrouter base_url must use https (got {})",
                    self.openrouter_base_url
                ),
            }
            .into());
        }

        if self.local_only {
            if is_remote_provider(&self.provider) {
                return Err(UserError::InvalidConfig {
                    reason: format!(
                        "local_only=true rejects remote STT provider '{}'\n  \
                         Hint: set [stt] provider = \"local\" or unset local_only",
                        self.provider
                    ),
                }
                .into());
            }
            if is_remote_provider(&self.tts_provider) {
                return Err(UserError::InvalidConfig {
                    reason: format!(
                        "local_only=true rejects remote TTS provider '{}'\n  \
                         Hint: set [tts] provider = \"local\" or unset local_only",
                        self.tts_provider
                    ),
                }
                .into());
            }
        }

        self.validate_tts_custom_models()?;
        Ok(())
    }

    /// Validate `[[tts.custom_models]]` uniqueness and reserved namespaces.
    pub fn validate_tts_custom_models(&self) -> Result<()> {
        #[cfg(feature = "tts")]
        {
            use crate::tts::{validate_custom_models, CustomTtsModelEntry, MAX_CUSTOM_MODELS};
            if self.tts_custom_models.len() > MAX_CUSTOM_MODELS {
                return Err(UserError::InvalidConfig {
                    reason: format!(
                        "too many [[tts.custom_models]] entries ({} > {MAX_CUSTOM_MODELS})",
                        self.tts_custom_models.len()
                    ),
                }
                .into());
            }
            let mut ids = std::collections::HashSet::new();
            let mut present = Vec::new();
            for e in &self.tts_custom_models {
                let id = e.id.trim();
                if id.is_empty() {
                    return Err(UserError::InvalidConfig {
                        reason: "custom TTS model id must be non-empty".into(),
                    }
                    .into());
                }
                if !ids.insert(id.to_string()) {
                    return Err(UserError::InvalidConfig {
                        reason: format!("duplicate custom TTS model id '{id}'"),
                    }
                    .into());
                }
                if id == crate::tts::DEFAULT_TTS_MODEL
                    || crate::tts::lookup_model(id)
                        .map(|m| m.shipped)
                        .unwrap_or(false)
                {
                    return Err(UserError::InvalidConfig {
                        reason: format!(
                            "custom model id '{id}' collides with built-in catalogue entry"
                        ),
                    }
                    .into());
                }
                let _ = crate::tts::lookup_adapter(&e.adapter)?;
                let trust = crate::tts::TrustMode::parse(&e.trust)?;
                if matches!(trust, crate::tts::TrustMode::Builtin) {
                    return Err(UserError::InvalidConfig {
                        reason: "custom models cannot use trust=builtin".into(),
                    }
                    .into());
                }
                if let Some(dir) = e.pack_dir.as_ref().map(PathBuf::from) {
                    if dir.exists() {
                        present.push(CustomTtsModelEntry {
                            id: e.id.clone(),
                            adapter: e.adapter.clone(),
                            pack_dir: e.pack_dir.clone(),
                            trust: e.trust.clone(),
                            license: e.license.clone(),
                            notes: e.notes.clone(),
                        });
                    }
                } else {
                    return Err(UserError::InvalidConfig {
                        reason: format!(
                            "custom model '{id}' requires pack_dir (remote custom packs \
                             are not enabled in v0.0.3)"
                        ),
                    }
                    .into());
                }
            }
            if !present.is_empty() {
                let _ = validate_custom_models(&present)?;
            }
        }
        Ok(())
    }

    /// Redacted diagnostic view for `--print-effective-config` (JOE-1608 / JOE-1935).
    pub fn effective_diagnostic(&self) -> EffectiveConfigDiagnostic {
        let sources = ConfigSourceMap::default_attribution(self);
        EffectiveConfigDiagnostic {
            provider: self.provider.clone(),
            model: self.model.clone(),
            language: self.language.clone(),
            output: self.output.clone(),
            timestamps: self.timestamps,
            openrouter_api_key: self.openrouter_api_key.as_ref().map(|_| "***".into()),
            openrouter_base_url: self.openrouter_base_url.clone(),
            openrouter_default_model: self.openrouter_default_model.clone(),
            openrouter_stt_mode: self.openrouter_stt_mode.clone(),
            openrouter_allow_custom_endpoint: self.openrouter_allow_custom_endpoint,
            providers: ProvidersDiagnostic {
                openrouter: ProviderSecretDiagnostic {
                    api_key: self.openrouter_api_key.as_ref().map(|_| "***".into()),
                    base_url: Some(self.openrouter_base_url.clone()),
                    api_key_source: sources.openrouter_api_key,
                },
                openai: ProviderSecretDiagnostic {
                    api_key: self.providers.openai.api_key.as_ref().map(|_| "***".into()),
                    base_url: self.providers.openai.base_url.clone(),
                    api_key_source: sources.openai_api_key,
                },
                elevenlabs: ProviderSecretDiagnostic {
                    api_key: self
                        .providers
                        .elevenlabs
                        .api_key
                        .as_ref()
                        .map(|_| "***".into()),
                    base_url: self.providers.elevenlabs.base_url.clone(),
                    api_key_source: sources.elevenlabs_api_key,
                },
                xai: ProviderSecretDiagnostic {
                    api_key: self.providers.xai.api_key.as_ref().map(|_| "***".into()),
                    base_url: self.providers.xai.base_url.clone(),
                    api_key_source: sources.xai_api_key,
                },
            },
            cleanup_style: self.cleanup_style.clone(),
            cleanup_provider: self.cleanup_provider.clone(),
            tts_provider: self.tts_provider.clone(),
            tts_model: self.tts_model.clone(),
            tts_voice: self.tts_voice.clone(),
            tts_language: self.tts_language.clone(),
            tts_speaking_rate: self.tts_speaking_rate,
            tts_max_chars: self.tts_max_chars,
            tts_timeout_ms: self.tts_timeout_ms,
            tts_pack_dir: self.tts_pack_dir.as_ref().map(|p| p.display().to_string()),
            tts_allow_unverified: self.tts_allow_unverified,
            tts_custom_model_ids: self
                .tts_custom_models
                .iter()
                .map(|m| m.id.clone())
                .collect(),
            local_only: self.local_only,
            config_path: self.config_path.as_ref().map(|p| p.display().to_string()),
            cache_dir: self.cache_dir.display().to_string(),
            sources,
        }
    }

    fn from_parts(file: Option<ConfigFile>, config_path: Option<PathBuf>) -> Result<Self> {
        let file = file.unwrap_or_default();

        let (provider, model, language, output) = resolve_stt(&file);
        let openrouter = resolve_openrouter(&file);

        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(SecretString::new)
            .or(openrouter.api_key);

        let openrouter_base_url = std::env::var("OPENROUTER_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(openrouter.base_url)
            .unwrap_or_else(|| DEFAULT_OPENROUTER_BASE_URL.to_string());

        let openrouter_default_model = openrouter
            .model
            .unwrap_or_else(|| DEFAULT_OPENROUTER_MODEL.to_string());

        let openai = merge_provider_cred(file.providers.openai.as_ref(), "OPENAI_API_KEY");
        let elevenlabs =
            merge_provider_cred(file.providers.elevenlabs.as_ref(), "ELEVENLABS_API_KEY");
        let xai = merge_provider_cred(file.providers.xai.as_ref(), "XAI_API_KEY");

        let cache_dir =
            Self::default_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("aurum-cache"));

        let tts_model = std::env::var("AURUM_TTS_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| file.tts.model.clone());
        let tts_voice = std::env::var("AURUM_TTS_VOICE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| file.tts.voice.clone());
        let tts_language = std::env::var("AURUM_TTS_LANGUAGE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| file.tts.language.clone());

        Ok(Self {
            provider,
            model: Some(model),
            language,
            output,
            output_file: None,
            timestamps: false,
            verbose: false,
            openrouter_api_key,
            openrouter_base_url,
            openrouter_default_model,
            openrouter_allow_custom_endpoint: openrouter.allow_custom_endpoint,
            openrouter_stt_mode: if openrouter.stt_mode.trim().is_empty() {
                default_stt_mode()
            } else {
                openrouter.stt_mode
            },
            openrouter_use_system_proxy: openrouter.use_system_proxy,
            providers: ProvidersConfig {
                openai,
                elevenlabs,
                xai,
            },
            cleanup_style: file.cleanup.style,
            cleanup_provider: file.cleanup.provider,
            cleanup_openrouter_model: file.cleanup.openrouter_model,
            tts_provider: file.tts.provider,
            tts_model,
            tts_voice,
            tts_language,
            tts_speaking_rate: file.tts.speaking_rate,
            tts_max_chars: file.tts.max_chars.max(1),
            tts_timeout_ms: if file.tts.timeout_ms == 0 {
                DEFAULT_TTS_TIMEOUT_MS
            } else {
                file.tts.timeout_ms
            },
            tts_pack_dir: file.tts.pack_dir.map(PathBuf::from),
            tts_allow_unverified: file.tts.allow_unverified,
            tts_custom_models: file.tts.custom_models,
            local_only: false,
            config_path,
            cache_dir,
        })
    }

    /// Apply CLI overrides on top of the loaded config.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_cli(
        &mut self,
        provider: Option<&str>,
        model: Option<&str>,
        language: Option<&str>,
        output: Option<&str>,
        output_file: Option<&Path>,
        timestamps: bool,
        verbose: bool,
        cleanup: Option<&str>,
        cleanup_provider: Option<&str>,
        cleanup_model: Option<&str>,
    ) {
        if let Some(p) = provider {
            self.provider = p.to_string();
        }
        if let Some(m) = model {
            self.model = Some(m.to_string());
        }
        if let Some(l) = language {
            self.language = l.to_string();
        }
        if let Some(o) = output {
            self.output = o.to_string();
        }
        if let Some(path) = output_file {
            self.output_file = Some(path.to_path_buf());
        }
        if timestamps {
            self.timestamps = true;
        }
        if verbose {
            self.verbose = true;
        }
        if let Some(c) = cleanup {
            self.cleanup_style = c.to_string();
        }
        if let Some(p) = cleanup_provider {
            self.cleanup_provider = p.to_string();
        }
        if let Some(m) = cleanup_model {
            self.cleanup_openrouter_model = Some(m.to_string());
        }
    }

    /// Resolve the effective model for the active provider.
    pub fn resolve_model(&self, model_explicitly_set: bool) -> Result<String> {
        if model_explicitly_set {
            let m = self
                .model
                .clone()
                .unwrap_or_else(|| self.default_model_for_provider());
            if self.provider == "openrouter"
                && !m.contains('/')
                && (crate::model::lookup_model(&m).is_ok() || m == DEFAULT_LOCAL_MODEL)
            {
                return Err(UserError::Other {
                    message: format!(
                        "model '{m}' looks like a local whisper model, not an OpenRouter id.\n \
 Hint: use e.g. google/gemini-2.5-flash-lite or openai/gpt-audio-mini, \
 or omit --model to use the OpenRouter default."
                    ),
                }
                .into());
            }
            return Ok(m);
        }
        match self.provider.as_str() {
            "openrouter" => {
                let m = self
                    .model
                    .clone()
                    .unwrap_or_else(|| self.openrouter_default_model.clone());
                if m.contains('/') {
                    Ok(m)
                } else if m == DEFAULT_LOCAL_MODEL || crate::model::lookup_model(&m).is_ok() {
                    Ok(self.openrouter_default_model.clone())
                } else {
                    Ok(m)
                }
            }
            _ => Ok(self
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string())),
        }
    }

    fn default_model_for_provider(&self) -> String {
        match self.provider.as_str() {
            "openrouter" => self.openrouter_default_model.clone(),
            _ => DEFAULT_LOCAL_MODEL.to_string(),
        }
    }
}

// --- Config load helpers (JOE-1935) ------------------------------------------

struct MergedOpenRouter {
    api_key: Option<SecretString>,
    model: Option<String>,
    base_url: Option<String>,
    allow_custom_endpoint: bool,
    stt_mode: String,
    use_system_proxy: bool,
}

/// Resolve `[stt]` (or built-in defaults).
fn resolve_stt(file: &ConfigFile) -> (String, String, String, String) {
    match file.stt.as_ref() {
        Some(s) => (
            s.provider.clone(),
            s.model.clone(),
            s.language.clone(),
            s.output.clone(),
        ),
        None => (
            default_provider(),
            default_local_model(),
            default_language(),
            default_output(),
        ),
    }
}

/// Resolve `[providers.openrouter]` (or empty defaults).
fn resolve_openrouter(file: &ConfigFile) -> MergedOpenRouter {
    match file.providers.openrouter.as_ref() {
        Some(s) => MergedOpenRouter {
            api_key: s.api_key.clone(),
            model: s.model.clone(),
            base_url: s.base_url.clone(),
            allow_custom_endpoint: s.allow_custom_endpoint,
            stt_mode: if s.stt_mode.trim().is_empty() {
                default_stt_mode()
            } else {
                s.stt_mode.clone()
            },
            use_system_proxy: s.use_system_proxy,
        },
        None => MergedOpenRouter {
            api_key: None,
            model: None,
            base_url: None,
            allow_custom_endpoint: false,
            stt_mode: default_stt_mode(),
            use_system_proxy: false,
        },
    }
}

fn merge_provider_cred(
    file: Option<&ProviderCredentialSection>,
    env_key: &str,
) -> ProviderCredentialConfig {
    let from_file = file.cloned().unwrap_or_default();
    let api_key = std::env::var(env_key)
        .ok()
        .filter(|s| !s.is_empty())
        .map(SecretString::new)
        .or(from_file.api_key);
    ProviderCredentialConfig {
        api_key,
        base_url: from_file.base_url,
    }
}

fn is_remote_provider(name: &str) -> bool {
    !matches!(name.to_ascii_lowercase().as_str(), "local" | "")
}

fn validate_stt_provider(name: &str) -> Result<()> {
    match name.to_ascii_lowercase().as_str() {
        "local" | "openrouter" | "openai" | "xai" => Ok(()),
        "elevenlabs" => Err(UserError::InvalidConfig {
            reason: "provider 'elevenlabs' is not valid for STT (TTS only)\n  \
                     Hint: use local, openrouter, openai, or xai for speech-to-text"
                .into(),
        }
        .into()),
        other => Err(UserError::InvalidProvider {
            provider: other.into(),
        }
        .into()),
    }
}

fn validate_tts_provider(name: &str) -> Result<()> {
    match name.to_ascii_lowercase().as_str() {
        "local" | "openrouter" | "openai" | "elevenlabs" | "xai" => Ok(()),
        other => Err(UserError::InvalidConfig {
            reason: format!(
                "unknown TTS provider '{other}'\n  \
                 Hint: use local, openrouter, openai, elevenlabs, or xai"
            ),
        }
        .into()),
    }
}

/// On-disk / pre-merge configuration schema (raw).
pub type RawConfig = ConfigFile;

/// Configuration that has passed [`Config::validate`] (JOE-1779 / JOE-1654).
#[derive(Clone)]
pub struct ValidatedConfig {
    inner: Config,
}

impl std::fmt::Debug for ValidatedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedConfig")
            .field("inner", &self.inner)
            .finish()
    }
}

impl ValidatedConfig {
    /// Validate and wrap a raw runtime config.
    pub fn try_from_config(cfg: Config) -> Result<Self> {
        cfg.validate()?;
        Ok(Self { inner: cfg })
    }

    /// Load defaults/file/env and validate.
    pub fn load() -> Result<Self> {
        Self::try_from_config(Config::load()?)
    }

    /// Load an optional path (missing → defaults) then validate.
    pub fn load_from(path: &Path) -> Result<Self> {
        Self::try_from_config(Config::load_from(path)?)
    }

    /// Load a required path then validate.
    pub fn load_from_required(path: &Path) -> Result<Self> {
        Self::try_from_config(Config::load_from_required(path)?)
    }

    pub fn as_config(&self) -> &Config {
        &self.inner
    }

    /// Consume into the underlying config (already validated).
    pub fn into_config(self) -> Config {
        self.inner
    }

    /// Provider-scoped secret for [`crate::provider_platform::ProviderBuildContext`] (JOE-1935).
    pub fn provider_secret(&self, id: &ProviderId) -> Option<SecretString> {
        self.inner.provider_secret(id)
    }

    /// Set offline policy and re-validate (fail closed on remote providers).
    pub fn with_local_only(mut self, local_only: bool) -> Result<Self> {
        self.inner.local_only = local_only;
        Self::try_from_config(self.inner)
    }

    /// Apply CLI overrides then re-validate (fail closed).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_cli(
        mut self,
        provider: Option<&str>,
        model: Option<&str>,
        language: Option<&str>,
        output: Option<&str>,
        output_file: Option<&Path>,
        timestamps: bool,
        verbose: bool,
        cleanup: Option<&str>,
        cleanup_provider: Option<&str>,
        cleanup_model: Option<&str>,
    ) -> Result<Self> {
        self.inner.apply_cli(
            provider,
            model,
            language,
            output,
            output_file,
            timestamps,
            verbose,
            cleanup,
            cleanup_provider,
            cleanup_model,
        );
        Self::try_from_config(self.inner)
    }
}

impl AsRef<Config> for ValidatedConfig {
    fn as_ref(&self) -> &Config {
        &self.inner
    }
}

impl std::ops::Deref for ValidatedConfig {
    type Target = Config;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Maximum config file size before TOML parse (JOE-1593).
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;

fn load_config_file(path: &Path) -> Result<ConfigFile> {
    let meta = fs::metadata(path).map_err(|e| UserError::InvalidConfig {
        reason: format!("failed to stat {}: {e}", path.display()),
    })?;
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(UserError::InvalidConfig {
            reason: format!(
                "config file {} is too large ({} > {MAX_CONFIG_BYTES} bytes)",
                path.display(),
                meta.len()
            ),
        }
        .into());
    }
    let contents = fs::read_to_string(path).map_err(|e| UserError::InvalidConfig {
        reason: format!("failed to read {}: {e}", path.display()),
    })?;
    toml::from_str(&contents).map_err(|e| {
        UserError::InvalidConfig {
            reason: format!("failed to parse {}: {e}", path.display()),
        }
        .into()
    })
}

fn dirs_home() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        return Ok(PathBuf::from(h));
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return Ok(PathBuf::from(h));
    }
    Err(UserError::InvalidConfig {
        reason: "could not determine home directory".into(),
    }
    .into())
}

/// Write a starter config file if one does not already exist.
pub fn write_example_config(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let example = r#"# Aurum configuration
# Environment variables take precedence over values in this file for secrets.
# Prefer OPENROUTER_API_KEY / OPENAI_API_KEY / ELEVENLABS_API_KEY / XAI_API_KEY
# over api_key fields below. Never put live credentials in this file.
# TTS: AURUM_TTS_MODEL, AURUM_TTS_VOICE, AURUM_TTS_LANGUAGE override [tts].
#
# Provider is never inferred from key presence alone — omit STT/TTS provider to stay local.
# Only canonical sections are accepted: [stt], [cleanup], [tts], [providers.*].

[stt]
provider = "local"
model = "base"
language = "auto"
# output = "txt" # txt | srt | json

[cleanup]
# style = "raw" # raw | clean | bullets | professional | summary
# provider = "rules" # rules (on-device) | openrouter
# openrouter_model = "google/gemini-2.5-flash"

[tts]
provider = "local"
# model = "kitten-nano-int8"
# voice = "Luna"
# language = "en"
# speaking_rate = 1.0
# max_chars = 5000
# timeout_ms = 120000

# [providers.openrouter]
# stt_mode = "auto"
# model = "google/gemini-2.5-flash"
# base_url = "https://openrouter.ai/api/v1"
# allow_custom_endpoint = false
# use_system_proxy = false

# [providers.openai]
# base_url = "https://api.openai.com/v1"

# [providers.elevenlabs]
# [providers.xai]
"#;
    fs::write(path, example)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// Serialize tests that mutate process environment (parallel cargo test).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Isolate env mutations for secret-related tests.
    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn clear(keys: &[&str]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut saved = Vec::new();
            for k in keys {
                saved.push((k.to_string(), std::env::var(k).ok()));
                // Safety: held under ENV_LOCK; single-threaded mutation of these keys.
                std::env::remove_var(k);
            }
            Self { saved, _lock: lock }
        }

        fn set(&self, key: &str, val: &str) {
            std::env::set_var(key, val);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in self.saved.drain(..) {
                match v {
                    Some(val) => std::env::set_var(&k, val),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }

    #[test]
    fn parses_config_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[stt]
provider = "openrouter"
model = "small"
language = "en"
output = "json"

[providers.openrouter]
api_key = "test-key"
model = "google/gemini-2.5-flash"
"#
        )
        .unwrap();

        let _g = EnvGuard::clear(&["OPENROUTER_API_KEY"]);
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.model.as_deref(), Some("small"));
        assert_eq!(cfg.language, "en");
        assert_eq!(cfg.output, "json");
        assert_eq!(
            cfg.openrouter_api_key.as_ref().map(|s| s.expose()),
            Some("test-key")
        );
        assert!(!format!("{:?}", cfg).contains("test-key"));
        assert_eq!(cfg.openrouter_default_model, "google/gemini-2.5-flash");
        assert_eq!(cfg.cleanup_style, "raw");
        assert_eq!(cfg.cleanup_provider, "rules");
        assert!(cfg.validate().is_ok());
        let diag = cfg.effective_diagnostic();
        assert_eq!(diag.openrouter_api_key.as_deref(), Some("***"));
        assert_eq!(diag.providers.openrouter.api_key.as_deref(), Some("***"));
    }

    #[test]
    fn load_from_required_missing_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let err = Config::load_from_required(&path).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn parses_cleanup_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[stt]
provider = "local"
model = "base"

[cleanup]
style = "clean"
provider = "rules"
openrouter_model = "google/gemini-2.5-flash"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.cleanup_style, "clean");
        assert_eq!(cfg.cleanup_provider, "rules");
        assert_eq!(
            cfg.cleanup_openrouter_model.as_deref(),
            Some("google/gemini-2.5-flash")
        );
    }

    #[test]
    fn cli_overrides_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[stt]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[cleanup]
style = "clean"
provider = "rules"
"#,
        )
        .unwrap();
        let mut cfg = Config::load_from(&path).unwrap();
        cfg.apply_cli(
            Some("openrouter"),
            Some("google/gemini-2.5-flash"),
            Some("fr"),
            Some("srt"),
            Some(Path::new("out.srt")),
            true,
            true,
            Some("summary"),
            Some("openrouter"),
            Some("openai/gpt-audio-mini"),
        );
        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.model.as_deref(), Some("google/gemini-2.5-flash"));
        assert_eq!(cfg.language, "fr");
        assert_eq!(cfg.output, "srt");
        assert_eq!(cfg.output_file.as_deref(), Some(Path::new("out.srt")));
        assert!(cfg.timestamps);
        assert!(cfg.verbose);
        assert_eq!(cfg.cleanup_style, "summary");
        assert_eq!(cfg.cleanup_provider, "openrouter");
        assert_eq!(
            cfg.cleanup_openrouter_model.as_deref(),
            Some("openai/gpt-audio-mini")
        );
    }

    #[test]
    #[cfg(feature = "tts")]
    fn custom_tts_model_cannot_shadow_builtin_on_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let default_id = crate::tts::DEFAULT_TTS_MODEL;
        fs::write(
            &path,
            format!(
                r#"
[tts]
model = "{default_id}"

[[tts.custom_models]]
id = "{default_id}"
adapter = "fake-sine-v1"
pack_dir = "/tmp/does-not-matter"
trust = "verified"
"#
            ),
        )
        .unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(
            err.to_string().contains("collides") || err.to_string().contains("reserved"),
            "got: {err}"
        );
    }

    #[test]
    fn defaults_when_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "local");
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.output, "txt");
        assert_eq!(cfg.cleanup_style, "raw");
        assert_eq!(cfg.cleanup_provider, "rules");
        assert_eq!(cfg.tts_provider, "local");
        assert!((cfg.tts_speaking_rate - 1.0).abs() < f32::EPSILON);
        assert!(!cfg.local_only);
    }

    #[test]
    fn key_presence_does_not_select_provider() {
        let _g = EnvGuard::clear(&[
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ELEVENLABS_API_KEY",
            "XAI_API_KEY",
        ]);
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[providers.openrouter]
api_key = "sk-or-present-but-ignored-for-selection"
[providers.openai]
api_key = "sk-openai-present"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(
            cfg.provider, "local",
            "STT must stay local without explicit provider"
        );
        assert_eq!(cfg.tts_provider, "local");
        assert!(cfg.openrouter_api_key.is_some());
        assert!(cfg.providers.openai.api_key.is_some());
    }

    #[test]
    fn unknown_top_level_section_fails() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[default]
provider = "local"
"#,
        )
        .unwrap();
        let err = Config::load_from(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("default") || msg.contains("Invalid"),
            "got: {msg}"
        );
    }

    #[test]
    fn new_stt_section_loads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[stt]
provider = "openrouter"
model = "google/gemini-2.5-flash"
language = "en"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.model.as_deref(), Some("google/gemini-2.5-flash"));
        assert_eq!(cfg.language, "en");
    }

    #[test]
    fn providers_openrouter_loads() {
        let _g = EnvGuard::clear(&["OPENROUTER_API_KEY", "OPENROUTER_BASE_URL"]);
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[providers.openrouter]
api_key = "from-providers"
model = "openai/gpt-audio-mini"
stt_mode = "transcriptions"
base_url = "https://openrouter.ai/api/v1"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(
            cfg.openrouter_api_key.as_ref().map(|s| s.expose()),
            Some("from-providers")
        );
        assert_eq!(cfg.openrouter_default_model, "openai/gpt-audio-mini");
        assert_eq!(cfg.openrouter_stt_mode, "transcriptions");
    }

    #[test]
    fn env_provider_keys_are_scoped() {
        let g = EnvGuard::clear(&[
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ELEVENLABS_API_KEY",
            "XAI_API_KEY",
        ]);
        g.set("OPENAI_API_KEY", "sk-openai-env");
        g.set("ELEVENLABS_API_KEY", "sk-el-env");
        g.set("XAI_API_KEY", "sk-xai-env");
        g.set("OPENROUTER_API_KEY", "sk-or-env");

        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "").unwrap();
        let cfg = Config::load_from(&path).unwrap();

        assert_eq!(cfg.provider, "local");
        assert_eq!(
            cfg.openrouter_api_key.as_ref().map(|s| s.expose()),
            Some("sk-or-env")
        );
        assert_eq!(
            cfg.providers.openai.api_key.as_ref().map(|s| s.expose()),
            Some("sk-openai-env")
        );
        assert_eq!(
            cfg.providers
                .elevenlabs
                .api_key
                .as_ref()
                .map(|s| s.expose()),
            Some("sk-el-env")
        );
        assert_eq!(
            cfg.providers.xai.api_key.as_ref().map(|s| s.expose()),
            Some("sk-xai-env")
        );

        let v = ValidatedConfig::try_from_config(cfg).unwrap();
        assert_eq!(
            v.provider_secret(&ProviderId::openrouter())
                .unwrap()
                .expose(),
            "sk-or-env"
        );
        assert_eq!(
            v.provider_secret(&ProviderId::must("openai"))
                .unwrap()
                .expose(),
            "sk-openai-env"
        );
        assert_eq!(
            v.provider_secret(&ProviderId::must("elevenlabs"))
                .unwrap()
                .expose(),
            "sk-el-env"
        );
        assert_eq!(
            v.provider_secret(&ProviderId::must("xai"))
                .unwrap()
                .expose(),
            "sk-xai-env"
        );
        assert!(v.provider_secret(&ProviderId::local()).is_none());
    }

    #[test]
    fn env_overrides_file_secret() {
        let g = EnvGuard::clear(&["OPENAI_API_KEY"]);
        g.set("OPENAI_API_KEY", "from-env");
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[providers.openai]
api_key = "from-file"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(
            cfg.providers.openai.api_key.as_ref().map(|s| s.expose()),
            Some("from-env")
        );
    }

    #[test]
    fn local_only_rejects_remote_stt() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.provider = "openrouter".into();
        cfg.local_only = true;
        let err = ValidatedConfig::try_from_config(cfg).unwrap_err();
        assert!(err.to_string().contains("local_only"), "got: {err}");
    }

    #[test]
    fn local_only_rejects_remote_tts() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.tts_provider = "elevenlabs".into();
        cfg.local_only = true;
        let err = ValidatedConfig::try_from_config(cfg).unwrap_err();
        assert!(err.to_string().contains("local_only"), "got: {err}");
    }

    #[test]
    fn local_only_allows_local_providers() {
        let dir = tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        let v = ValidatedConfig::try_from_config(cfg)
            .unwrap()
            .with_local_only(true)
            .unwrap();
        assert!(v.local_only);
        assert_eq!(v.provider, "local");
        assert_eq!(v.tts_provider, "local");
    }

    #[test]
    fn unknown_provider_section_fails_closed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[providers.notarealvendor]
api_key = "x"
"#,
        )
        .unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(
            err.to_string().contains("parse") || err.to_string().contains("unknown"),
            "got: {err}"
        );
    }

    #[test]
    fn redacted_debug_and_diagnostic() {
        let _g = EnvGuard::clear(&[
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ELEVENLABS_API_KEY",
            "XAI_API_KEY",
        ]);
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[providers.openrouter]
api_key = "sk-or-canary-secret-value-xyz"
[providers.openai]
api_key = "sk-openai-canary-secret-value"
[providers.elevenlabs]
api_key = "sk-el-canary-secret-value"
[providers.xai]
api_key = "sk-xai-canary-secret-value"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("sk-or-canary"));
        assert!(!dbg.contains("sk-openai-canary"));
        assert!(!dbg.contains("sk-el-canary"));
        assert!(!dbg.contains("sk-xai-canary"));

        let diag = cfg.effective_diagnostic();
        let json = serde_json::to_string(&diag).unwrap();
        assert!(!json.contains("sk-or-canary"));
        assert!(!json.contains("sk-openai-canary"));
        assert_eq!(diag.providers.openai.api_key.as_deref(), Some("***"));
        assert_eq!(diag.providers.elevenlabs.api_key.as_deref(), Some("***"));
        assert_eq!(diag.providers.xai.api_key.as_deref(), Some("***"));
        assert_eq!(diag.providers.openrouter.api_key.as_deref(), Some("***"));
    }

    #[test]
    fn tts_speaking_rate_and_provider() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tts]
provider = "local"
speaking_rate = 1.25
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert!((cfg.tts_speaking_rate - 1.25).abs() < 0.001);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn invalid_speaking_rate_rejected() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.tts_speaking_rate = 0.0;
        assert!(cfg.validate().is_err());
        cfg.tts_speaking_rate = 10.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn elevenlabs_invalid_for_stt() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.provider = "elevenlabs".into();
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("elevenlabs") || err.to_string().contains("STT"));
    }

    #[test]
    fn validated_config_accepts_defaults() {
        let dir = tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        let v = ValidatedConfig::try_from_config(cfg).unwrap();
        assert_eq!(v.provider, "local");
        assert_eq!(v.as_config().language, "auto");
    }

    #[test]
    fn validated_config_rejects_bad_provider() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.provider = "not-a-provider".into();
        let err = ValidatedConfig::try_from_config(cfg).unwrap_err();
        assert!(
            err.to_string().contains("provider") || err.to_string().contains("Invalid"),
            "got: {err}"
        );
    }

    #[test]
    fn validated_apply_cli_revalidates() {
        let dir = tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        let v = ValidatedConfig::try_from_config(cfg).unwrap();
        let err = v
            .apply_cli(
                Some("bogus"),
                None,
                None,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("provider") || err.to_string().contains("Invalid"));
    }

    #[test]
    fn example_config_contains_no_live_credential_placeholder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("example.toml");
        write_example_config(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sk-or-v1-"));
        assert!(!text.contains("sk-proj-"));
        assert!(text.contains("[stt]"));
        assert!(text.contains("[providers.openrouter]") || text.contains("providers.openrouter"));
    }
}
