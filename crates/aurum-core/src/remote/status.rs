//! Secret redaction helpers for remote diagnostics (JOE-1914 / retest JOE-1920).

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

    // Authorization: header form (value after colon). Byte-safe whitespace skip.
    let mut cursor = 0;
    while cursor < out.len() {
        let lower = out[cursor..].to_ascii_lowercase();
        let Some(rel) = lower.find("authorization:") else {
            break;
        };
        let idx = cursor + rel;
        let after = idx + "authorization:".len();
        // Sum UTF-8 byte lengths of leading whitespace — never use char count as offset.
        let ws_bytes: usize = out[after..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(|c| c.len_utf8())
            .sum();
        let value_start = after + ws_bytes;
        if value_start > out.len() {
            break;
        }
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

/// Closed, **locally defined** provider code set that may appear in public errors.
///
/// Anything else from the remote JSON `code` field is dropped (JOE-1920 retest of F-001).
/// Character-class “looks safe” validation is intentionally **not** used for strings.
const CLOSED_PROVIDER_STRING_CODES: &[&str] = &[
    // Stable, non-secret labels we choose to surface (not provider free text).
    "rate_limit_exceeded",
    "insufficient_quota",
    "invalid_request",
    "invalid_api_key",
    "model_not_found",
    "context_length_exceeded",
    "server_error",
    "timeout",
    "not_found",
];

/// Extract a public-safe provider code from a JSON body, if any.
///
/// - Integer codes are allowed only when they fit in a small non-negative range
///   used as coarse categories (not free-form secrets).
/// - String codes must match [`CLOSED_PROVIDER_STRING_CODES`] exactly (case-sensitive).
/// - Arbitrary alphanumeric “codes” (including credential-shaped strings) are dropped.
pub fn extract_allowlisted_provider_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = v
        .get("error")
        .and_then(|e| e.get("code"))
        .or_else(|| v.get("code"))?;

    if let Some(n) = code.as_u64() {
        // Coarse HTTP-like / OpenAPI style numeric codes only.
        if n <= 599 {
            return Some(n.to_string());
        }
        return None;
    }
    if let Some(n) = code.as_i64() {
        if (0..=599).contains(&n) {
            return Some(n.to_string());
        }
        return None;
    }
    if let Some(s) = code.as_str() {
        let t = s.trim();
        if CLOSED_PROVIDER_STRING_CODES.contains(&t) {
            return Some(t.to_string());
        }
        // Unknown or free-form string codes are never echoed.
        return None;
    }
    None
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

    const CANARY: &str = "sk-or-v1-TESTCANARY-JOE1920-DO-NOT-USE-001";

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
    fn authorization_redaction_handles_unicode_whitespace_without_panic() {
        // NBSP (U+00A0) after the colon — must not use char-count as byte offset.
        let s = format!("Authorization:\u{00a0}Bearer {CANARY}");
        let out = redact_secret(&s);
        assert!(!out.contains("TESTCANARY"));
        assert!(!out.contains(CANARY));
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

    #[test]
    fn provider_code_channel_drops_credential_shaped_string() {
        // High retest failure: character-class codes leaked secrets.
        let body = format!(r#"{{"error":{{"message":"ignored","code":"{CANARY}"}}}}"#);
        let r = public_http_reason(401, &body);
        assert!(
            !r.contains("TESTCANARY") && !r.contains("sk-or-v1"),
            "credential-shaped provider code leaked: {r}"
        );
        assert_eq!(r, "HTTP 401");
    }

    #[test]
    fn provider_code_channel_drops_unknown_alphanumeric_string() {
        let body = r#"{"error":{"message":"x","code":"no_endpoints_available_xyz"}}"#;
        let r = public_http_reason(404, body);
        assert!(!r.contains("no_endpoints"));
        assert_eq!(r, "HTTP 404");
    }

    #[test]
    fn provider_code_allows_closed_string_set_only() {
        let body = r#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#;
        let r = public_http_reason(429, body);
        assert!(r.contains("provider_code=rate_limit_exceeded"));
        assert!(!r.contains("slow down"));
    }

    #[test]
    fn provider_code_drops_oversized_numeric() {
        let body = r#"{"error":{"code":999999,"message":"nope"}}"#;
        let r = public_http_reason(500, body);
        assert_eq!(r, "HTTP 500");
    }
}
