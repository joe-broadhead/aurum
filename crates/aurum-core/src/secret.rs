//! Redacting secret wrapper (JOE-1779 / JOE-1654).
//!
//! Secrets must never appear via Debug, Display, or accidental logging.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque secret string. Debug/Display always print `***`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Access the raw secret. Prefer not logging the return value.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
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
}
