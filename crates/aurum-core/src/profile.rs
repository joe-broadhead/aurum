//! Intent-oriented quality profiles (JOE-1723).
//!
//! Profiles resolve to verified catalogue model IDs using a **versioned evidence
//! mapping**. They never select experimental models (e.g. `large-v3-q5_0`).
//! Explicit `--model` always wins over a profile.
//!
//! The default product model remains `base` until an evidence review changes it;
//! profiles are opt-in intents, not silent default rewrites.

use crate::error::{Result, UserError};
use crate::model::{lookup_model, model_support_tier, ModelSupportTier};
use serde::{Deserialize, Serialize};

/// Evidence pack version tied to published eval methodology (JOE-2216).
///
/// Tracks the STT quality observatory evidence pack. Bump when mappings or
/// the reviewed baseline change; requires a changelog entry and budget review.
/// See `evals/observatory/` and [`crate::eval::STT_OBSERVATORY_EVIDENCE_VERSION`].
pub const PROFILE_EVIDENCE_VERSION: &str = "0.0.22-observatory-v1";

/// User-facing quality intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityProfile {
    /// Prefer low download size and load time (trial / constrained machines).
    Speed,
    /// Default product intent (maps to the historical `base` default family).
    Balance,
    /// Prefer higher accuracy among supported catalogue models (not experimental).
    Quality,
}

impl QualityProfile {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "speed" | "fast" => Ok(Self::Speed),
            "balance" | "balanced" | "default" => Ok(Self::Balance),
            "quality" | "accurate" | "high" => Ok(Self::Quality),
            other => Err(UserError::Other {
                message: format!(
                    "unknown quality profile '{other}'\n  \
                     Hint: use speed | balance | quality"
                ),
            }
            .into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Balance => "balance",
            Self::Quality => "quality",
        }
    }
}

/// Deterministic profile → model resolution with full transparency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileResolution {
    pub profile: String,
    pub model: String,
    pub evidence_version: String,
    pub language: String,
    pub reasons: Vec<String>,
    pub alternatives: Vec<String>,
    pub approx_bytes: u64,
    pub support_tier: String,
}

/// Resolve a profile for the given language preference (`auto`, `en`, …).
///
/// Rules:
/// - Never select [`ModelSupportTier::Experimental`].
/// - Prefer English-only quantised models when language is clearly English.
/// - Network is never required; resolution is pure catalogue lookup.
pub fn resolve_profile(profile: QualityProfile, language: &str) -> Result<ProfileResolution> {
    let lang = language.trim().to_ascii_lowercase();
    let english = lang == "en" || lang == "eng" || lang.starts_with("en-");

    let (model, reasons): (&str, Vec<String>) = match profile {
        QualityProfile::Speed => {
            if english {
                (
                    "tiny.en-q5_1",
                    vec![
                        "speed profile prefers smallest supported download".into(),
                        "language is English → english-only quantised tiny".into(),
                    ],
                )
            } else {
                (
                    "tiny-q5_1",
                    vec![
                        "speed profile prefers smallest supported download".into(),
                        "language is not fixed English → multilingual tiny-q5_1".into(),
                    ],
                )
            }
        }
        QualityProfile::Balance => {
            if english {
                (
                    "base.en",
                    vec![
                        "balance profile tracks the product default family".into(),
                        "language is English → base.en".into(),
                        format!(
                            "evidence_version={PROFILE_EVIDENCE_VERSION} does not change global default"
                        ),
                    ],
                )
            } else {
                (
                    "base",
                    vec![
                        "balance profile tracks the product default model `base`".into(),
                        format!("evidence_version={PROFILE_EVIDENCE_VERSION} (STT observatory)"),
                    ],
                )
            }
        }
        QualityProfile::Quality => {
            if english {
                (
                    "small.en",
                    vec![
                        "quality profile selects next supported size above base".into(),
                        "language is English → small.en".into(),
                        "experimental large-v3-q5_0 is never selected by profiles".into(),
                    ],
                )
            } else {
                (
                    "small",
                    vec![
                        "quality profile selects next supported size above base".into(),
                        "experimental large-v3-q5_0 is never selected by profiles".into(),
                    ],
                )
            }
        }
    };

    let info = lookup_model(model)?;
    match model_support_tier(info.name) {
        ModelSupportTier::Supported => {}
        ModelSupportTier::Experimental => {
            return Err(UserError::Other {
                message: format!(
                    "internal error: profile resolved experimental model '{}'",
                    info.name
                ),
            }
            .into());
        }
    }

    let candidates: &[&str] = match profile {
        QualityProfile::Speed => &["tiny", "tiny-q8_0", "base-q5_1"],
        QualityProfile::Balance => &["base-q5_1", "base-q8_0", "small-q5_1"],
        QualityProfile::Quality => &["small-q5_1", "medium", "large-v3-turbo"],
    };
    let alternatives: Vec<String> = candidates
        .iter()
        .copied()
        .filter(|m| lookup_model(m).is_ok() && model_support_tier(m) == ModelSupportTier::Supported)
        .filter(|m| *m != info.name)
        .map(str::to_string)
        .collect();

    Ok(ProfileResolution {
        profile: profile.as_str().into(),
        model: info.name.into(),
        evidence_version: PROFILE_EVIDENCE_VERSION.into(),
        language: language.into(),
        reasons,
        alternatives,
        approx_bytes: info.approx_bytes,
        support_tier: "supported".into(),
    })
}

/// Format a human-readable recommendation block.
pub fn format_recommendation(res: &ProfileResolution) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "profile={} → model={}  (tier={}, ~{} MB)\n",
        res.profile,
        res.model,
        res.support_tier,
        res.approx_bytes / 1_000_000
    ));
    out.push_str(&format!("evidence_version={}\n", res.evidence_version));
    out.push_str(&format!("language={}\n", res.language));
    out.push_str("reasons:\n");
    for r in &res.reasons {
        out.push_str(&format!("  - {r}\n"));
    }
    if !res.alternatives.is_empty() {
        out.push_str(&format!("alternatives: {}\n", res.alternatives.join(", ")));
    }
    out.push_str("override: pass --model <id> to force a catalogue model\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_never_experimental() {
        for lang in ["auto", "en", "fr"] {
            let r = resolve_profile(QualityProfile::Speed, lang).unwrap();
            assert_eq!(model_support_tier(&r.model), ModelSupportTier::Supported);
            assert!(!r.model.contains("large-v3-q5_0"));
        }
    }

    #[test]
    fn balance_defaults_to_base_family() {
        let r = resolve_profile(QualityProfile::Balance, "auto").unwrap();
        assert_eq!(r.model, "base");
        let en = resolve_profile(QualityProfile::Balance, "en").unwrap();
        assert_eq!(en.model, "base.en");
    }

    #[test]
    fn quality_uses_small_not_experimental_large() {
        let r = resolve_profile(QualityProfile::Quality, "auto").unwrap();
        assert_eq!(r.model, "small");
        assert_ne!(r.model, "large-v3-q5_0");
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(
            QualityProfile::parse("fast").unwrap(),
            QualityProfile::Speed
        );
        assert_eq!(
            QualityProfile::parse("balanced").unwrap(),
            QualityProfile::Balance
        );
    }
}
