//! Configuration loading for Aurum.
//!
//! Precedence (highest wins):
//! 1. CLI flags
//! 2. Environment variables for OpenRouter only (`OPENROUTER_API_KEY`, `OPENROUTER_BASE_URL`)
//! 3. Config file (`~/.config/aurum/config.toml` on Linux; platform-appropriate elsewhere)
//! 4. Built-in defaults

use crate::error::{Result, UserError};
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

/// On-disk configuration file schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub default: DefaultSection,
    #[serde(default)]
    pub openrouter: OpenRouterSection,
    #[serde(default)]
    pub cleanup: CleanupSection,
    #[serde(default)]
    pub tts: TtsSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultSection {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_local_model")]
    pub model: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_output")]
    pub output: String,
}

impl Default for DefaultSection {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_local_model(),
            language: default_language(),
            output: default_output(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenRouterSection {
    /// Prefer `OPENROUTER_API_KEY` env var over this field.
    pub api_key: Option<String>,
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

fn default_stt_mode() -> String {
    "auto".into()
}

/// Post-ASR cleanup defaults ( flow).
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

/// Local TTS defaults (`[tts]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsSection {
    /// Only `local` in MVP.
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    #[serde(default = "default_tts_model")]
    pub model: String,
    #[serde(default = "default_tts_voice")]
    pub voice: String,
    #[serde(default = "default_tts_language")]
    pub language: String,
    #[serde(default = "default_tts_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_tts_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for TtsSection {
    fn default() -> Self {
        Self {
            provider: default_tts_provider(),
            model: default_tts_model(),
            voice: default_tts_voice(),
            language: default_tts_language(),
            max_chars: default_tts_max_chars(),
            timeout_ms: default_tts_timeout_ms(),
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

/// Fully-resolved runtime configuration after merging all sources.
#[derive(Clone)]
pub struct Config {
    pub provider: String,
    pub model: Option<String>,
    pub language: String,
    pub output: String,
    pub output_file: Option<PathBuf>,
    pub timestamps: bool,
    pub verbose: bool,
    pub openrouter_api_key: Option<String>,
    pub openrouter_base_url: String,
    pub openrouter_default_model: String,
    /// Allow custom credentialed endpoints (JOE-1587).
    pub openrouter_allow_custom_endpoint: bool,
    /// `auto` | `chat` | `transcriptions` (JOE-1586).
    pub openrouter_stt_mode: String,
    pub openrouter_use_system_proxy: bool,
    /// Cleanup style name (`raw`, `clean`, …).
    pub cleanup_style: String,
    /// Cleanup backend name (`rules`, `openrouter`).
    pub cleanup_provider: String,
    /// Optional dedicated model for OpenRouter cleanup.
    pub cleanup_openrouter_model: Option<String>,
    /// TTS provider name (`local` only in MVP).
    pub tts_provider: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub tts_language: String,
    pub tts_max_chars: usize,
    pub tts_timeout_ms: u64,
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
            .field(
                "openrouter_api_key",
                &self.openrouter_api_key.as_ref().map(|_| "***"),
            )
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
            .field("cleanup_style", &self.cleanup_style)
            .field("cleanup_provider", &self.cleanup_provider)
            .field("cleanup_openrouter_model", &self.cleanup_openrouter_model)
            .field("tts_provider", &self.tts_provider)
            .field("tts_model", &self.tts_model)
            .field("tts_voice", &self.tts_voice)
            .field("tts_language", &self.tts_language)
            .field("tts_max_chars", &self.tts_max_chars)
            .field("tts_timeout_ms", &self.tts_timeout_ms)
            .field("config_path", &self.config_path)
            .field("cache_dir", &self.cache_dir)
            .finish()
    }
}

impl Config {
    /// Resolve the platform-appropriate config file path.
    pub fn default_config_path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "aurum").map(|d| d.config_dir().join("config.toml"))
    }

    /// Resolve the platform-appropriate cache directory (models live under `models/`).
    pub fn default_cache_dir() -> Result<PathBuf> {
        // Prefer XDG-style cache via the `directories` crate.
        if let Some(dirs) = ProjectDirs::from("", "", "aurum") {
            return Ok(dirs.cache_dir().to_path_buf());
        }
        // Fallback: ~/.cache/aurum
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
        Ok(Self::from_parts(file, path))
    }

    /// Load from an explicit config file path (used in tests).
    pub fn load_from(path: &Path) -> Result<Self> {
        let file = if path.exists() {
            Some(load_config_file(path)?)
        } else {
            None
        };
        Ok(Self::from_parts(file, Some(path.to_path_buf())))
    }

    fn from_parts(file: Option<ConfigFile>, config_path: Option<PathBuf>) -> Self {
        let file = file.unwrap_or_default();

        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or(file.openrouter.api_key.clone());

        let openrouter_base_url = std::env::var("OPENROUTER_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(file.openrouter.base_url.clone())
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());

        let openrouter_default_model = file
            .openrouter
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_OPENROUTER_MODEL.to_string());

        let cache_dir =
            Self::default_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("aurum-cache"));

        // Env overrides for TTS (no secrets).
        let tts_model = std::env::var("AURUM_TTS_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or(file.tts.model);
        let tts_voice = std::env::var("AURUM_TTS_VOICE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or(file.tts.voice);
        let tts_language = std::env::var("AURUM_TTS_LANGUAGE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or(file.tts.language);

        Self {
            provider: file.default.provider,
            model: Some(file.default.model),
            language: file.default.language,
            output: file.default.output,
            output_file: None,
            timestamps: false,
            verbose: false,
            openrouter_api_key,
            openrouter_base_url,
            openrouter_default_model,
            openrouter_allow_custom_endpoint: file.openrouter.allow_custom_endpoint,
            openrouter_stt_mode: if file.openrouter.stt_mode.trim().is_empty() {
                default_stt_mode()
            } else {
                file.openrouter.stt_mode
            },
            openrouter_use_system_proxy: file.openrouter.use_system_proxy,
            cleanup_style: file.cleanup.style,
            cleanup_provider: file.cleanup.provider,
            cleanup_openrouter_model: file.cleanup.openrouter_model,
            tts_provider: file.tts.provider,
            tts_model,
            tts_voice,
            tts_language,
            tts_max_chars: file.tts.max_chars.max(1),
            tts_timeout_ms: if file.tts.timeout_ms == 0 {
                DEFAULT_TTS_TIMEOUT_MS
            } else {
                file.tts.timeout_ms
            },
            config_path,
            cache_dir,
        }
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
    ///
    /// When `model_explicitly_set` is false and the provider is openrouter, a bare
    /// local whisper name (e.g. config default `base`) is replaced with the
    /// openrouter default model id.
    /// Resolve model for the active provider.
    ///
    /// Returns `Err` if OpenRouter is selected with an explicit bare local whisper name
    /// (e.g. `--provider openrouter --model tiny`) — that cannot be a remote model id.
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
# Environment variables take precedence over values in this file.
# OPENROUTER_API_KEY is preferred over openrouter.api_key below.
# TTS: AURUM_TTS_MODEL, AURUM_TTS_VOICE, AURUM_TTS_LANGUAGE override [tts].

[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[cleanup]
# style = "raw" # raw | clean | bullets | professional | summary
# provider = "rules" # rules (on-device) | openrouter
# openrouter_model = "google/gemini-2.5-flash"

[tts]
# provider = "local"
# model = "kitten-nano-int8"
# voice = "Luna"
# language = "en"
# max_chars = 5000
# timeout_ms = 120000

[openrouter]
# api_key = "sk-or-..."
# model = "google/gemini-2.5-flash"
# base_url = "https://openrouter.ai/api/v1"
"#;
    fs::write(path, example)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_config_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[default]
provider = "openrouter"
model = "small"
language = "en"
output = "json"

[openrouter]
api_key = "test-key"
model = "google/gemini-2.5-flash"
"#
        )
        .unwrap();

        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.model.as_deref(), Some("small"));
        assert_eq!(cfg.language, "en");
        assert_eq!(cfg.output, "json");
        assert_eq!(cfg.openrouter_api_key.as_deref(), Some("test-key"));
        assert_eq!(cfg.openrouter_default_model, "google/gemini-2.5-flash");
        assert_eq!(cfg.cleanup_style, "raw");
        assert_eq!(cfg.cleanup_provider, "rules");
    }

    #[test]
    fn parses_cleanup_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[default]
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
[default]
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
    fn defaults_when_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "local");
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.output, "txt");
        assert_eq!(cfg.cleanup_style, "raw");
        assert_eq!(cfg.cleanup_provider, "rules");
    }
}
