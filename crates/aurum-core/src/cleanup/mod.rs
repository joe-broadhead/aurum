//! Post-transcription text cleanup ( "flow").
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
use crate::runtime::OpContext;
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
 "unknown cleanup style '{other}'\n Hint: use one of: raw, clean, bullets, professional, summary"
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
                    "unknown cleanup provider '{other}'\n Hint: use one of: rules, openrouter"
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
 "unknown segment cleanup policy '{other}'\n Hint: use one of: auto, keep, clear, per-segment"
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

/// Result of a cleanup pass over bare text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub text: String,
    pub style: CleanupStyle,
    pub provider: CleanupProviderKind,
    /// Original text before cleanup (for debugging / JSON).
    pub original_text: String,
}

/// Structured report for a full-result cleanup transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub style: CleanupStyle,
    pub provider: CleanupProviderKind,
    pub segment_policy: SegmentCleanupPolicy,
    /// Fields that changed relative to the input result.
    pub changed_fields: Vec<String>,
    pub warnings: Vec<String>,
    /// Segments dropped because rewritten text was empty (per-segment policy).
    pub dropped_segments: usize,
    /// Whether timings were cleared under the applied policy.
    pub segments_cleared: bool,
}

/// Hard bounds for per-segment cleanup (failure semantics live here; remote
/// batching belongs to a follow-up issue).
pub const MAX_PER_SEGMENT_COUNT: usize = 2_000;
pub const MAX_PER_SEGMENT_CHARS: usize = 8_000;

/// Pluggable cleanup backend.
#[async_trait]
pub trait TextCleanup: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> CleanupProviderKind;

    async fn cleanup(&self, text: &str, style: CleanupStyle) -> Result<CleanupResult>;

    /// Clean many segment texts transactionally (JOE-1832).
    ///
    /// Default: sequential single-text calls. Remote backends may batch with
    /// stable indices and must return only after all work succeeds.
    async fn cleanup_segments(&self, texts: &[&str], style: CleanupStyle) -> Result<Vec<String>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            out.push(self.cleanup(t, style).await?.text);
        }
        Ok(out)
    }
}

/// Apply cleanup to a full transcription result.
///
/// Updates `text` and cleanup metadata. Segment handling follows `segments` policy.
pub async fn apply_cleanup(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
) -> Result<CleanupResult> {
    let (out, _report) =
        apply_cleanup_with_segments(result, cleanup, style, SegmentCleanupPolicy::Auto).await?;
    Ok(out)
}

/// Like [`apply_cleanup`] with explicit segment policy.
///
/// **Transactional:** the caller's `TranscriptionResult` is mutated only after
/// the full operation validates. An injected failure on segment N leaves the
/// original result byte-for-byte unchanged.
pub async fn apply_cleanup_with_segments(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
    segments: SegmentCleanupPolicy,
) -> Result<(CleanupResult, CleanupReport)> {
    apply_cleanup_with_segments_op(result, cleanup, style, segments, None).await
}

/// Like [`apply_cleanup_with_segments`] with an optional [`OpContext`] for cancel,
/// deadline, and progress (JOE-1831).
pub async fn apply_cleanup_with_segments_op(
    result: &mut TranscriptionResult,
    cleanup: &dyn TextCleanup,
    style: CleanupStyle,
    segments: SegmentCleanupPolicy,
    op: Option<&OpContext>,
) -> Result<(CleanupResult, CleanupReport)> {
    let owned_op;
    let op = match op {
        Some(o) => o,
        None => {
            owned_op = OpContext::new();
            &owned_op
        }
    };
    let policy = segments.resolve(style);
    op.emit(
        "cleanup",
        format!("style={} policy={policy:?}", style.as_str()),
    );
    op.check()?;

    if matches!(style, CleanupStyle::Raw) {
        // Raw remains the identity default — no metadata pollution.
        result.cleanup_style = CleanupStyle::Raw;
        result.cleanup_provider = None;
        result.original_text = None;
        result.original_segments = None;
        result.cleanup_segment_policy = None;
        let out = CleanupResult {
            text: result.text.clone(),
            style,
            provider: cleanup.kind(),
            original_text: result.text.clone(),
        };
        let report = CleanupReport {
            style,
            provider: cleanup.kind(),
            segment_policy: policy,
            changed_fields: vec![],
            warnings: vec![],
            dropped_segments: 0,
            segments_cleared: false,
        };
        return Ok((out, report));
    }

    // Bound per-segment work before any mutation.
    if matches!(policy, SegmentCleanupPolicy::PerSegment) {
        if result.segments.len() > MAX_PER_SEGMENT_COUNT {
            return Err(UserError::Other {
                message: format!(
                    "per-segment cleanup refused: {} segments exceeds limit of {MAX_PER_SEGMENT_COUNT}",
                    result.segments.len()
                ),
            }
            .into());
        }
        for (i, seg) in result.segments.iter().enumerate() {
            let n = seg.text().chars().count();
            if n > MAX_PER_SEGMENT_CHARS {
                return Err(UserError::Other {
                    message: format!(
                        "per-segment cleanup refused: segment {i} has {n} chars \
                         (limit {MAX_PER_SEGMENT_CHARS})"
                    ),
                }
                .into());
            }
        }
    }

    // Snapshot inputs; mutate only after full success.
    let original_text = result.text.clone();
    let original_segments = result.segments.clone();
    let mut warnings = Vec::new();
    let mut dropped_segments = 0usize;
    let mut segments_cleared = false;

    op.emit("cleanup", "full_text");
    let out = cleanup.cleanup(&original_text, style).await?;
    op.check()?;

    let proposed_segments = match policy {
        SegmentCleanupPolicy::Keep | SegmentCleanupPolicy::Auto => {
            // Keep timings; TXT may diverge from SRT segment text for light styles.
            if matches!(style, CleanupStyle::Clean | CleanupStyle::Professional) {
                warnings.push(
                    "segment timings kept; segment text is pre-cleanup ASR while \
                     `text` is cleaned (JSON exposes original_text)"
                        .into(),
                );
            }
            original_segments.clone()
        }
        SegmentCleanupPolicy::Clear => {
            segments_cleared = true;
            warnings.push(
                "segments cleared under structural/explicit clear policy; \
                 use original_segments for raw ASR timings"
                    .into(),
            );
            Vec::new()
        }
        SegmentCleanupPolicy::PerSegment => {
            op.emit("cleanup", "per_segment");
            // Transactional: no host mutation until every segment cleans successfully
            // (JOE-1832). OpenRouter batches remotely with stable ids.
            let refs: Vec<&str> = original_segments.iter().map(|s| s.text()).collect();
            let cleaned_texts =
                cleanup
                    .cleanup_segments(&refs, style)
                    .await
                    .map_err(|e| UserError::Other {
                        message: format!("per-segment cleanup failed (transaction aborted): {e}"),
                    })?;
            if cleaned_texts.len() != original_segments.len() {
                return Err(UserError::Other {
                    message: format!(
                        "per-segment cleanup returned {} texts for {} segments",
                        cleaned_texts.len(),
                        original_segments.len()
                    ),
                }
                .into());
            }
            let mut cleaned = Vec::with_capacity(original_segments.len());
            for (seg, text) in original_segments.iter().zip(cleaned_texts) {
                let mut seg = seg.clone();
                seg.set_text(text);
                if seg.text().trim().is_empty() {
                    dropped_segments += 1;
                } else {
                    cleaned.push(seg);
                }
            }
            cleaned
        }
    };

    // Commit proposed state only now.
    op.emit("cleanup", "commit");
    let mut changed_fields = vec![
        "text".into(),
        "cleanup_style".into(),
        "cleanup_provider".into(),
    ];
    result.original_text = Some(original_text.clone());
    result.original_segments = Some(original_segments);
    result.text = out.text.clone();
    result.cleanup_style = out.style;
    result.cleanup_provider = Some(out.provider);
    result.cleanup_segment_policy = Some(policy);
    if result.segments != proposed_segments {
        changed_fields.push("segments".into());
    }
    result.segments = proposed_segments;
    changed_fields.push("original_text".into());
    changed_fields.push("original_segments".into());

    let report = CleanupReport {
        style: out.style,
        provider: out.provider,
        segment_policy: policy,
        changed_fields,
        warnings,
        dropped_segments,
        segments_cleared,
    };
    Ok((out, report))
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
        let out = c.cleanup(" keep this ", CleanupStyle::Raw).await.unwrap();
        assert_eq!(out.text, "keep this");
    }

    #[tokio::test]
    async fn structural_auto_clears_segments() {
        let c = RulesCleanup::new();
        let mut result = TranscriptionResult::local(
            "One. Two. Three.".into(),
            vec![
                Segment::from_parts_unchecked(0.0, 1.0, "One.".to_string()),
                Segment::from_parts_unchecked(1.0, 2.0, "Two.".to_string()),
            ],
            Some("en".into()),
            "tiny".into(),
            2.0,
        );
        let (_out, report) = apply_cleanup_with_segments(
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
        assert_eq!(result.cleanup_provider, Some(CleanupProviderKind::Rules));
        assert!(report.segments_cleared);
        assert!(result.original_segments.as_ref().unwrap().len() == 2);
        assert_eq!(result.original_text.as_deref(), Some("One. Two. Three."));
    }

    #[tokio::test]
    async fn keep_preserves_segments() {
        let c = RulesCleanup::new();
        let mut result = TranscriptionResult::local(
            "um hello there".into(),
            vec![Segment::from_parts_unchecked(
                0.0,
                1.0,
                "um hello there".to_string(),
            )],
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
        assert_eq!(result.segments[0].text(), "um hello there");
        assert!(result.original_text.is_some());
    }

    /// Failing backend used only for transactional guarantees.
    struct FailAfterN {
        n: std::sync::atomic::AtomicUsize,
        fail_at: usize,
        inner: RulesCleanup,
    }

    #[async_trait]
    impl TextCleanup for FailAfterN {
        fn name(&self) -> &'static str {
            "fail-after-n"
        }
        fn kind(&self) -> CleanupProviderKind {
            CleanupProviderKind::Rules
        }
        async fn cleanup(&self, text: &str, style: CleanupStyle) -> Result<CleanupResult> {
            let i = self.n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if i >= self.fail_at {
                return Err(UserError::Other {
                    message: format!("injected failure at call {i}"),
                }
                .into());
            }
            self.inner.cleanup(text, style).await
        }
    }

    #[tokio::test]
    async fn per_segment_failure_leaves_result_unchanged() {
        let backend = FailAfterN {
            n: std::sync::atomic::AtomicUsize::new(0),
            // Full-transcript cleanup succeeds (call 0); first segment rewrite fails (call 1).
            fail_at: 1,
            inner: RulesCleanup::new(),
        };
        let mut result = TranscriptionResult::local(
            "um one. um two.".into(),
            vec![
                Segment::from_parts_unchecked(0.0, 1.0, "um one.".to_string()),
                Segment::from_parts_unchecked(1.0, 2.0, "um two.".to_string()),
            ],
            None,
            "tiny".into(),
            2.0,
        );
        let before = serde_json::to_string(&result).unwrap();
        let err = apply_cleanup_with_segments(
            &mut result,
            &backend,
            CleanupStyle::Clean,
            SegmentCleanupPolicy::PerSegment,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("segment"), "{err}");
        let after = serde_json::to_string(&result).unwrap();
        assert_eq!(before, after, "result must be unchanged after failure");
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
