//! Secret redaction helpers for remote diagnostics.

/// Redact bearer tokens / API keys from diagnostic strings.
pub fn redact_secret(s: &str) -> String {
    let mut out = s.to_string();
    // Common patterns: Bearer sk-..., api_key=..., Authorization headers echoed.
    for needle in ["sk-or-", "sk-ant-", "Bearer "] {
        if let Some(idx) = out.find(needle) {
            let start = idx;
            let end = out[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|i| start + i)
                .unwrap_or(out.len());
            if end > start + needle.len() + 4 {
                out.replace_range(start..end, &format!("{needle}***"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openrouter_key() {
        let s = redact_secret("Authorization: Bearer sk-or-v1-abcdef0123456789");
        assert!(!s.contains("abcdef"));
        assert!(s.contains("***"));
    }
}
