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

    /// Styles that rewrite structure so ASR segment timings no longer match text.
    pub fn is_structural(self) -> bool {
        matches!(self, Self::Bullets | Self::Summary)
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

/// How to treat ASR segments after cleaning the full transcript text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SegmentCleanupPolicy {
    /// Leave segments unchanged (best for timed SRT with `clean` / `professional`).
    Keep,
    /// Drop all segments (default for structural styles: bullets / summary).
    Clear,
    /// Run the same cleanup style on each segment's text (keeps timings).
    PerSegment,
    /// Pick a sensible default from the style (see [`default_for_style`](Self::default_for_style)).
    #[default]
    Auto,
}

impl SegmentCleanupPolicy {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "keep" | "none" => Ok(Self::Keep),
            "clear" | "drop" | "empty" => Ok(Self::Clear),
            "per-segment" | "per_segment" | "each" | "segments" => Ok(Self::PerSegment),
            "auto" | "default" => Ok(Self::Auto),
            other => Err(UserError::Other {
                message: format!(
                    "unknown segment cleanup policy '{other}'\n  Hint: use one of: auto, keep, clear, per-segment"
                ),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Clear => "clear",
            Self::PerSegment => "per-segment",
            Self::Auto => "auto",
        }
    }

    /// Resolve `Auto` against a cleanup style.
    pub fn resolve(self, style: CleanupStyle) -> Self {
        match self {
            Self::Auto => Self::default_for_style(style),
            other => other,
        }
    }

    /// Structural styles clear segments; light styles keep them.
    pub fn default_for_style(style: CleanupStyle) -> Self {
        if style.is_structural() {
            Self::Clear
        } else {
            Self::Keep
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

/// Apply cleanup to a full transcription result.
///
/// Updates `text` and cleanup metadata. Segment handling follows `segments` policy.
pub async fn apply_cleanup(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
) -> Result<CleanupResult> {
    apply_cleanup_with_segments(result, cleanup, style, SegmentCleanupPolicy::Auto).await
}

/// Like [`apply_cleanup`] with explicit segment policy.
pub async fn apply_cleanup_with_segments(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
    segments: SegmentCleanupPolicy,
) -> Result<CleanupResult> {
    let policy = segments.resolve(style);

    if matches!(style, CleanupStyle::Raw) {
        result.cleanup_style = CleanupStyle::Raw;
        result.cleanup_provider = None;
        result.original_text = None;
        return Ok(CleanupResult {
            text: result.text.clone(),
            style,
            provider: cleanup.kind(),
            original_text: result.text.clone(),
        });
    }

    let out = cleanup.cleanup(&result.text, style).await?;
    result.original_text = Some(out.original_text.clone());
    result.text = out.text.clone();
    result.cleanup_style = out.style;
    result.cleanup_provider = Some(out.provider);

    match policy {
        SegmentCleanupPolicy::Keep | SegmentCleanupPolicy::Auto => {}
        SegmentCleanupPolicy::Clear => {
            result.segments.clear();
        }
        SegmentCleanupPolicy::PerSegment => {
            // For structural styles, per-segment cleanup is usually wrong; still honor request.
            let mut cleaned = Vec::with_capacity(result.segments.len());
            for mut seg in result.segments.drain(..) {
                let piece = cleanup.cleanup(&seg.text, style).await?;
                seg.text = piece.text;
                if !seg.text.trim().is_empty() {
                    cleaned.push(seg);
                }
            }
            result.segments = cleaned;
        }
    }

    Ok(out)
}

/// Clean a bare string (stdin / text file path) without a full transcription result.
pub async fn cleanup_text(
    text: &str,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
) -> Result<CleanupResult> {
    if matches!(style, CleanupStyle::Raw) {
        return Ok(CleanupResult {
            text: text.trim().to_string(),
            style,
            provider: cleanup.kind(),
            original_text: text.to_string(),
        });
    }
    cleanup.cleanup(text, style).await
}

pub use openrouter::OpenRouterCleanup;
pub use rules::RulesCleanup;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Segment;

    #[tokio::test]
    async fn rules_clean_strips_fillers() {
        let c = RulesCleanup::new();
        let out = c
            .cleanup(
                "um, hello there, you know, this is a test",
                CleanupStyle::Clean,
            )
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
        let out = c
            .cleanup("  keep this  ", CleanupStyle::Raw)
            .await
            .unwrap();
        assert_eq!(out.text, "keep this");
    }

    #[tokio::test]
    async fn structural_auto_clears_segments() {
        let c = RulesCleanup::new();
        let mut result = TranscriptionResult::local(
            "One. Two. Three.".into(),
            vec![
                Segment {
                    start: 0.0,
                    end: 1.0,
                    text: "One.".into(),
                },
                Segment {
                    start: 1.0,
                    end: 2.0,
                    text: "Two.".into(),
                },
            ],
            Some("en".into()),
            "tiny".into(),
            2.0,
        );
        apply_cleanup_with_segments(
            &mut result,
            &c,
            CleanupStyle::Bullets,
            SegmentCleanupPolicy::Auto,
        )
        .await
        .unwrap();
        assert!(result.segments.is_empty());
        assert!(result.text.contains('•'));
        assert_eq!(result.cleanup_style, CleanupStyle::Bullets);
        assert_eq!(
            result.cleanup_provider,
            Some(CleanupProviderKind::Rules)
        );
    }

    #[tokio::test]
    async fn keep_preserves_segments() {
        let c = RulesCleanup::new();
        let mut result = TranscriptionResult::local(
            "um hello there".into(),
            vec![Segment {
                start: 0.0,
                end: 1.0,
                text: "um hello there".into(),
            }],
            None,
            "tiny".into(),
            1.0,
        );
        apply_cleanup_with_segments(
            &mut result,
            &c,
            CleanupStyle::Clean,
            SegmentCleanupPolicy::Keep,
        )
        .await
        .unwrap();
        assert_eq!(result.segments.len(), 1);
    }

    #[tokio::test]
    async fn cleanup_text_standalone() {
        let c = RulesCleanup::new();
        let out = cleanup_text("um, hi there", &c, CleanupStyle::Clean)
            .await
            .unwrap();
        assert!(!out.text.to_ascii_lowercase().contains("um"));
    }
}
