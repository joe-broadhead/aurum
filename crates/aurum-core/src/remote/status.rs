//! Secret redaction helpers for remote diagnostics (JOE-1914).

use crate::secret::SecretString;

/// Redact bearer tokens / API keys from diagnostic strings.
///
/// Strategy (defense in depth):
/// 1. Replace every known secret value (exact match), longest first.
/// 2. Replace common token-shaped prefixes case-insensitively, **all** occurrences.
///
/// Prefer passing configured secrets via [`redact_secret_with`] so redaction does
/// not depend only on prefix guessing.
pub fn redact_secret(s: &str) -> String {
    redact_secret_with(s, &[])
}

/// Like [`redact_secret`], also scrubbing the provided known secret values.
pub fn redact_secret_with(s: &str, known_secrets: &[&str]) -> String {
    let mut out = s.to_string();

    // 1) Exact known values (longest first to avoid partial overlaps).
    let mut known: Vec<&str> = known_secrets
        .iter()
        .copied()
        .filter(|k| k.len() >= SecretString::MIN_REDACT_LEN)
        .collect();
    known.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for secret in known {
        if out.contains(secret) {
            out = out.replace(secret, "***");
        }
    }

    // 2) Prefix patterns — case-insensitive, all occurrences.
    // Order: longer / more specific first.
    const NEEDLES: &[&str] = &["sk-or-", "sk-ant-", "bearer ", "api_key=", "api-key="];

    loop {
        let lower = out.to_ascii_lowercase();
        let mut best: Option<(usize, usize, &'static str)> = None;
        for needle in NEEDLES {
            if let Some(idx) = lower.find(needle) {
                let start = idx;
                let value_start = start + needle.len();
                let end = out[value_start..]
                    .find(|c: char| {
                        c.is_whitespace()
                            || c == '"'
                            || c == '\''
                            || c == ','
                            || c == '}'
                            || c == '&'
                            || c == ';'
                    })
                    .map(|i| value_start + i)
                    .unwrap_or(out.len());
                // Require material after the prefix.
                if end > value_start + 4 {
                    // Prefer earliest match; on tie, longer needle.
                    let take = match best {
                        None => true,
                        Some((b_start, _, _)) => idx < b_start,
                    };
                    if take {
                        best = Some((start, end, needle));
                    }
                }
            }
        }
        let Some((start, end, needle)) = best else {
            break;
        };
        let replacement = match needle {
            "bearer " => "Bearer ***",
            "api_key=" => "api_key=***",
            "api-key=" => "api-key=***",
            _ => "***",
        };
        out.replace_range(start..end, replacement);
    }

    // Authorization: header form (value after colon). Scan once left-to-right.
    let mut cursor = 0;
    while cursor < out.len() {
        let lower = out[cursor..].to_ascii_lowercase();
        let Some(rel) = lower.find("authorization:") else {
            break;
        };
        let idx = cursor + rel;
        let after = idx + "authorization:".len();
        let ws = out[after..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        let value_start = after + ws;
        if out[value_start..].starts_with("***") {
            cursor = value_start + 3;
            continue;
        }
        let end = out[value_start..]
            .find(['\n', '\r', '"', '\''])
            .map(|i| value_start + i)
            .unwrap_or(out.len());
        if end <= value_start {
            break;
        }
        out.replace_range(idx..end, "Authorization: ***");
        cursor = idx + "Authorization: ***".len();
    }

    out
}

/// Extract a short allowlisted provider error code from a JSON body, if present.
///
/// Only returns compact alphanumeric / underscore / hyphen tokens (≤ 64 chars).
/// Never returns free-form message text.
pub fn extract_allowlisted_provider_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = v
        .get("error")
        .and_then(|e| e.get("code"))
        .or_else(|| v.get("code"))?;
    if let Some(s) = code.as_str() {
        return sanitize_code(s);
    }
    if let Some(n) = code.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = code.as_u64() {
        return Some(n.to_string());
    }
    None
}

fn sanitize_code(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.len() > 64 {
        return None;
    }
    if t.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        Some(t.to_string())
    } else {
        None
    }
}

/// Build a public, allowlisted remote-error reason (no provider body echo).
pub fn public_http_reason(status: u16, body: &str) -> String {
    match extract_allowlisted_provider_code(body) {
        Some(code) => format!("HTTP {status} (provider_code={code})"),
        None => format!("HTTP {status}"),
    }
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

    #[test]
    fn redacts_all_occurrences_case_insensitive() {
        let s = redact_secret("bearer sk-or-v1-AAAA1111 and BEARER sk-or-v1-BBBB2222");
        assert!(!s.contains("AAAA"));
        assert!(!s.contains("BBBB"));
        assert!(s.to_ascii_lowercase().contains("bearer ***"));
    }

    #[test]
    fn redacts_known_secret_without_prefix() {
        let canary = "totally-unique-canary-secret-99";
        let s = redact_secret_with(&format!("leaked={canary} in body"), &[canary]);
        assert!(!s.contains("unique-canary"));
        assert!(s.contains("***"));
    }

    #[test]
    fn public_reason_no_body_echo() {
        let body = r#"{"error":{"message":"sk-or-v1-secretvaluehere","code":404}}"#;
        let r = public_http_reason(404, body);
        assert!(!r.contains("sk-or"));
        assert!(!r.contains("secretvalue"));
        assert!(r.contains("404"));
        assert!(r.contains("provider_code=404"));
    }

    #[test]
    fn public_reason_without_json() {
        let r = public_http_reason(500, "internal boom with sk-or-v1-abcdef");
        assert_eq!(r, "HTTP 500");
        assert!(!r.contains("sk-or"));
    }
}
