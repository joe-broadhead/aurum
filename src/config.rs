//! Configuration loading for Aurum.
//!
//! Precedence (highest wins):
//! 1. CLI flags
//! 2. Environment variables
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

/// On-disk configuration file schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub default: DefaultSection,
    #[serde(default)]
    pub openrouter: OpenRouterSection,
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

/// Fully-resolved runtime configuration after merging all sources.
#[derive(Debug, Clone)]
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
    pub config_path: Option<PathBuf>,
    pub cache_dir: PathBuf,
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

        let cache_dir = Self::default_cache_dir()
            .unwrap_or_else(|_| PathBuf::from(std::env::temp_dir()).join("aurum-cache"));

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
            config_path,
            cache_dir,
        }
    }

    /// Apply CLI overrides on top of the loaded config.
    pub fn apply_cli(
        &mut self,
        provider: Option<&str>,
        model: Option<&str>,
        language: Option<&str>,
        output: Option<&str>,
        output_file: Option<&Path>,
        timestamps: bool,
        verbose: bool,
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
    }

    /// Effective model name for the selected provider.
    pub fn effective_model(&self) -> String {
        if let Some(m) = &self.model {
            // If the user left the generic default "base" while on openrouter, swap to
            // the openrouter default unless they explicitly set a model via CLI already.
            // Simpler rule: if provider is openrouter and model looks like a local whisper
            // short name (no slash), use openrouter default only when model equals local default
            // AND it came from defaults. For v0 we just return what's set; CLI layer picks
            // the right default per provider.
            return m.clone();
        }
        match self.provider.as_str() {
            "openrouter" => self.openrouter_default_model.clone(),
            _ => DEFAULT_LOCAL_MODEL.to_string(),
        }
    }
}

fn load_config_file(path: &Path) -> Result<ConfigFile> {
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

[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

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
        );
        assert_eq!(cfg.provider, "openrouter");
        assert_eq!(cfg.model.as_deref(), Some("google/gemini-2.5-flash"));
        assert_eq!(cfg.language, "fr");
        assert_eq!(cfg.output, "srt");
        assert_eq!(cfg.output_file.as_deref(), Some(Path::new("out.srt")));
        assert!(cfg.timestamps);
        assert!(cfg.verbose);
    }

    #[test]
    fn defaults_when_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.provider, "local");
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.output, "txt");
    }
}
