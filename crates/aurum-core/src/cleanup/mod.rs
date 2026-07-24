//! Post-transcription text cleanup (Zephyr-style "flow").
//!
//! Two backends:
//! - **rules** (default): pure on-device regex/heuristics — no network, no model
//! - **openrouter**: LLM rewrite via chat completions (optional, explicit)
//!
//! Cleanup is **opt-in** at the CLI/library boundary so raw ASR stays available.

pub mod openrouter;
pub mod rules;

use crate::error::{Result, UserError};
use crate::providers::TranscriptionResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// How aggressively / in what shape to clean transcript text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStyle {
    /// No cleanup (identity).
    #[default]
    Raw,
    /// Collapse whitespace, drop fillers, light punctuation.
    Clean,
    /// Clean + bullet list.
    Bullets,
    /// Clean + expand contractions / slightly more formal tone (rules) or LLM polish.
    Professional,
    /// Extractive (rules) or abstractive (LLM) short summary.
    Summary,
}

impl CleanupStyle {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "raw" | "none" | "off" => Ok(Self::Raw),
            "clean" => Ok(Self::Clean),
            "bullets" | "bullet" => Ok(Self::Bullets),
            "professional" | "pro" => Ok(Self::Professional),
            "summary" | "sum" => Ok(Self::Summary),
            other => Err(UserError::Other {
                message: format!(
                    "unknown cleanup style '{other}'\n  Hint: use one of: raw, clean, bullets, professional, summary"
                ),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Clean => "clean",
            Self::Bullets => "bullets",
            Self::Professional => "professional",
            Self::Summary => "summary",
        }
    }
}

/// Where cleanup runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CleanupProviderKind {
    /// On-device rules only (default).
    #[default]
    Rules,
    /// OpenRouter chat completion.
    OpenRouter,
}

impl CleanupProviderKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rules" | "local" | "on-device" | "ondevice" => Ok(Self::Rules),
            "openrouter" | "remote" | "llm" => Ok(Self::OpenRouter),
            other => Err(UserError::Other {
                message: format!(
                    "unknown cleanup provider '{other}'\n  Hint: use one of: rules, openrouter"
                ),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::OpenRouter => "openrouter",
        }
    }
}

/// Result of a cleanup pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub text: String,
    pub style: CleanupStyle,
    pub provider: CleanupProviderKind,
    /// Original text before cleanup (for debugging / JSON).
    pub original_text: String,
}

/// Pluggable cleanup backend.
#[async_trait]
pub trait TextCleanup: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> CleanupProviderKind;

    async fn cleanup(&self, text: &str, style: CleanupStyle) -> Result<CleanupResult>;
}

/// Apply cleanup to a full transcription result (updates `text` only).
///
/// Segments are left unchanged for ASR alignment; hosts that need cleaned
/// segment text should re-run cleanup per segment or drop segments for
/// summary/bullets styles.
pub async fn apply_cleanup(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
) -> Result<CleanupResult> {
    if matches!(style, CleanupStyle::Raw) {
        return Ok(CleanupResult {
            text: result.text.clone(),
            style,
            provider: cleanup.kind(),
            original_text: result.text.clone(),
        });
    }
    let out = cleanup.cleanup(&result.text, style).await?;
    result.text = out.text.clone();
    Ok(out)
}

pub use openrouter::OpenRouterCleanup;
pub use rules::RulesCleanup;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rules_clean_strips_fillers() {
        let c = RulesCleanup::new();
        let out = c
            .cleanup("um, hello there, you know, this is a test", CleanupStyle::Clean)
            .await
            .unwrap();
        let lower = out.text.to_ascii_lowercase();
        assert!(!lower.contains(" um"));
        assert!(lower.contains("hello"));
        assert!(lower.contains("test"));
    }

    #[tokio::test]
    async fn rules_bullets() {
        let c = RulesCleanup::new();
        let out = c
            .cleanup(
                "First point here. Second point there. Third idea now.",
                CleanupStyle::Bullets,
            )
            .await
            .unwrap();
        assert!(out.text.contains('•'));
        assert!(out.text.lines().count() >= 2);
    }

    #[tokio::test]
    async fn raw_trims_only() {
        let c = RulesCleanup::new();
        let out = c.cleanup("  keep this  ", CleanupStyle::Raw).await.unwrap();
        assert_eq!(out.text, "keep this");
    }
}
