//! End-to-end synthetic canary matrix for secret-free surfaces (JOE-1914 / JOE-1920).
//!
//! Canaries are synthetic only — never real credentials.

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::doctor::run_doctor;
    use crate::remote::client::map_http_status;
    use crate::remote::status::{public_http_reason, redact_secret_with};
    use crate::secret::SecretString;
    use crate::support::build_support_bundle;
    use reqwest::StatusCode;
    use tempfile::tempdir;

    /// Synthetic OpenRouter-shaped canary (JOE-1920 retest).
    const CANARY: &str = "sk-or-v1-TESTCANARY-JOE1920-DO-NOT-USE-001";

    fn assert_no_canary(surface: &str, text: &str) {
        assert!(
            !text.contains(CANARY) && !text.contains("TESTCANARY"),
            "canary leaked on {surface}: {}",
            text.chars().take(200).collect::<String>()
        );
    }

    #[test]
    fn canary_matrix_config_debug_and_serde() {
        let s = SecretString::new(CANARY);
        assert_no_canary("SecretString Debug", &format!("{s:?}"));
        assert_no_canary("SecretString Display", &format!("{s}"));
        let json = serde_json::to_string(&s).unwrap();
        assert_no_canary("SecretString Serialize", &json);
        assert_eq!(json, "\"***\"");
    }

    #[test]
    fn canary_matrix_doctor_and_support_bundle() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.openrouter_api_key = Some(SecretString::new(CANARY));

        let report = run_doctor(&cfg);
        let doctor_json = report.to_json_pretty().unwrap();
        assert_no_canary("doctor JSON", &doctor_json);

        let bundle = build_support_bundle(&cfg, Some(format!("notes with {CANARY}")));
        let bundle_json = bundle.to_json_pretty().unwrap();
        assert_no_canary("support bundle", &bundle_json);
        // Known-secret redaction helper must scrub when provided.
        let scrubbed = redact_secret_with(&format!("header {CANARY} trailer"), &[CANARY]);
        assert_no_canary("redact_secret_with", &scrubbed);
    }

    #[test]
    fn canary_matrix_http_errors_message_and_code_channels() {
        // Message field must never appear.
        let body_msg = format!(r#"{{"error":{{"message":"{CANARY}","code":401}}}}"#);
        let err = map_http_status("openrouter", StatusCode::UNAUTHORIZED, &body_msg).unwrap_err();
        assert_no_canary("error message channel", &err.to_string());

        // Code field with credential-shaped string must never appear (retest).
        let body_code = format!(r#"{{"error":{{"message":"x","code":"{CANARY}"}}}}"#);
        let err2 = map_http_status("openrouter", StatusCode::FORBIDDEN, &body_code).unwrap_err();
        assert_no_canary("error code channel", &err2.to_string());
        assert_eq!(public_http_reason(403, &body_code), "HTTP 403");
    }

    #[test]
    fn canary_matrix_effective_config_diagnostic() {
        let dir = tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.openrouter_api_key = Some(SecretString::new(CANARY));
        let diag = cfg.effective_diagnostic();
        let json = serde_json::to_string(&diag).unwrap();
        assert_no_canary("effective_diagnostic", &json);
        assert_eq!(diag.openrouter_api_key.as_deref(), Some("***"));
    }
}
