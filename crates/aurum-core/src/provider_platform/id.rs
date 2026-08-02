//! Validated provider identity (JOE-1933).

use crate::error::{Result, UserError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Maximum length of a provider id (bytes after normalization).
pub const MAX_PROVIDER_ID_LEN: usize = 32;

/// Stable, validated provider identity used at core boundaries.
///
/// Canonical form is lowercase ASCII `[a-z][a-z0-9_]*` with no leading digit,
/// no control characters, and no path/URL separators.
///
/// Serde deserialization always runs [`ProviderId::parse`] (JOE-1979) — never
/// transparent `String` construction that could bypass reserved-name / syntax checks.
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ProviderId(String);

impl ProviderId {
    /// Parse and normalize a provider id (or known alias).
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(invalid("provider id must not be empty"));
        }
        if trimmed.len() > MAX_PROVIDER_ID_LEN {
            return Err(invalid(&format!(
                "provider id exceeds {MAX_PROVIDER_ID_LEN} characters"
            )));
        }
        if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(invalid(
                "provider id must not contain whitespace or control characters",
            ));
        }

        let lowered = trimmed.to_ascii_lowercase();
        let canonical = match lowered.as_str() {
            // Explicit alias → canonical id (not a second implementation).
            "grok" => "xai".to_string(),
            other => other.to_string(),
        };

        if !is_valid_canonical(&canonical) {
            return Err(invalid(
                "provider id must match [a-z][a-z0-9_]* (lowercase letters, digits, underscore)",
            ));
        }

        // Reserved for future platform surfaces; not user-selectable providers.
        if matches!(
            canonical.as_str(),
            "auto" | "default" | "none" | "null" | "any" | "all" | "system"
        ) {
            return Err(invalid(&format!("provider id '{canonical}' is reserved")));
        }

        Ok(Self(canonical))
    }

    /// Infallible constructor for compile-time known ids (panics if invalid).
    pub fn must(raw: &str) -> Self {
        Self::parse(raw).unwrap_or_else(|e| panic!("invalid ProviderId {raw:?}: {e}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn local() -> Self {
        Self::must("local")
    }

    pub fn openrouter() -> Self {
        Self::must("openrouter")
    }
}

fn is_valid_canonical(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn invalid(reason: &str) -> crate::error::TranscriptionError {
    UserError::InvalidConfig {
        reason: reason.into(),
    }
    .into()
}

impl fmt::Debug for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProviderId").field(&self.0).finish()
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for ProviderId {
    type Err = crate::error::TranscriptionError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ProviderId::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_ids() {
        assert_eq!(ProviderId::parse("local").unwrap().as_str(), "local");
        assert_eq!(
            ProviderId::parse("OpenRouter").unwrap().as_str(),
            "openrouter"
        );
        assert_eq!(ProviderId::parse("xai").unwrap().as_str(), "xai");
        assert_eq!(
            ProviderId::parse("elevenlabs").unwrap().as_str(),
            "elevenlabs"
        );
    }

    #[test]
    fn alias_grok_maps_to_xai() {
        assert_eq!(ProviderId::parse("grok").unwrap().as_str(), "xai");
        assert_eq!(ProviderId::parse("Grok").unwrap().as_str(), "xai");
    }

    #[test]
    fn rejects_empty_and_controls() {
        assert!(ProviderId::parse("").is_err());
        assert!(ProviderId::parse("   ").is_err());
        assert!(ProviderId::parse("loc al").is_err());
        assert!(ProviderId::parse("loc\nal").is_err());
        // Leading/trailing whitespace is trimmed; internal newlines remain invalid.
        assert_eq!(ProviderId::parse("  local\n").unwrap().as_str(), "local");
        assert!(ProviderId::parse("a/b").is_err());
        assert!(ProviderId::parse("../x").is_err());
        assert!(ProviderId::parse("1bad").is_err());
        assert!(ProviderId::parse("-dash").is_err());
    }

    #[test]
    fn rejects_reserved() {
        assert!(ProviderId::parse("auto").is_err());
        assert!(ProviderId::parse("default").is_err());
        assert!(ProviderId::parse("none").is_err());
    }

    #[test]
    fn rejects_oversized() {
        let s = "a".repeat(MAX_PROVIDER_ID_LEN + 1);
        assert!(ProviderId::parse(&s).is_err());
    }

    #[test]
    fn serde_deserialize_uses_parse() {
        let id: ProviderId = serde_json::from_str("\"OpenRouter\"").unwrap();
        assert_eq!(id.as_str(), "openrouter");
        let id: ProviderId = serde_json::from_str("\"grok\"").unwrap();
        assert_eq!(id.as_str(), "xai");
        assert!(serde_json::from_str::<ProviderId>("\"auto\"").is_err());
        assert!(serde_json::from_str::<ProviderId>("\"a/b\"").is_err());
        assert!(serde_json::from_str::<ProviderId>("\"1bad\"").is_err());
        assert!(serde_json::from_str::<ProviderId>("\"loc al\"").is_err());
    }
}
