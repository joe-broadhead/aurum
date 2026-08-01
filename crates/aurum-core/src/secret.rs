//! Redacting secret wrapper (JOE-1779 / JOE-1914).
//!
//! Secrets must never appear via Debug, Display, or accidental Serde serialization.
//! Loading from config still deserializes the real value; serializing always emits
//! a redacted placeholder so dumps cannot leak credentials.

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Opaque secret string.
///
/// - [`Debug`] / [`Display`] always print a redacted form.
/// - [`Serialize`] always emits `"***"` (never the raw value).
/// - [`Deserialize`] accepts a string and stores it (config/env load path).
/// - Raw access is only via [`SecretString::expose`] / [`SecretString::into_inner`].
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Access the raw secret for Authorization-header construction only.
    /// Prefer not logging the return value.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Minimum length before a value is treated as a secret for redaction helpers.
    pub const MIN_REDACT_LEN: usize = 8;
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Never emit plaintext. Diagnostics and accidental serde dumps stay clean.
        serializer.serialize_str("***")
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl Visitor<'_> for V {
            type Value = SecretString;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a secret string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(SecretString::new(v))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(SecretString::new(v))
            }
        }
        deserializer.deserialize_string(V)
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_in_debug_or_display() {
        let s = SecretString::new("sk-super-secret");
        let d = format!("{s:?}");
        let disp = format!("{s}");
        assert!(!d.contains("sk-super-secret"));
        assert!(!disp.contains("sk-super-secret"));
        assert!(d.contains("***"));
        assert_eq!(s.expose(), "sk-super-secret");
    }

    #[test]
    fn serialize_never_emits_plaintext() {
        let s = SecretString::new("sk-or-v1-canary-plaintext-value-xyz");
        let json = serde_json::to_string(&s).expect("serialize");
        assert!(!json.contains("canary"));
        assert!(!json.contains("plaintext"));
        assert_eq!(json, "\"***\"");
    }

    #[test]
    fn deserialize_loads_value() {
        let s: SecretString = serde_json::from_str("\"sk-loaded-from-config\"").unwrap();
        assert_eq!(s.expose(), "sk-loaded-from-config");
        assert!(!format!("{s:?}").contains("sk-loaded"));
    }

    #[test]
    fn option_serialize_redacts() {
        let v: Option<SecretString> = Some(SecretString::new("sk-nested-secret-value"));
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("nested-secret"));
        assert!(json.contains("***"));
    }
}
