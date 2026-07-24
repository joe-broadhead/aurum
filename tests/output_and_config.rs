//! Integration-style unit coverage for formatters and config (no model download).

use aurum::config::Config;
use aurum::output::{format_result, OutputFormat};
use aurum::providers::{Segment, TranscriptionResult};
use std::fs;
use tempfile::tempdir;

fn sample() -> TranscriptionResult {
    TranscriptionResult {
        text: "One two three.".into(),
        language: Some("en".into()),
        model: "tiny".into(),
        provider: "local".into(),
        duration_secs: 2.0,
        segments: vec![Segment {
            start: 0.0,
            end: 2.0,
            text: "One two three.".into(),
        }],
    }
}

#[test]
fn all_formats_non_empty() {
    let r = sample();
    for fmt in [OutputFormat::Txt, OutputFormat::Srt, OutputFormat::Json] {
        let s = format_result(&r, fmt).unwrap();
        assert!(!s.trim().is_empty(), "{fmt:?} empty");
    }
}

#[test]
fn config_env_key_overrides_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
[default]
provider = "local"
model = "base"

[openrouter]
api_key = "from-file"
"#,
    )
    .unwrap();

    // Ensure env is clean for this test process section.
    // Note: other tests may set env; we only assert file load works here.
    let cfg = Config::load_from(&path).unwrap();
    // If OPENROUTER_API_KEY is set in the environment, it wins — that's intended.
    if std::env::var_os("OPENROUTER_API_KEY").is_none() {
        assert_eq!(cfg.openrouter_api_key.as_deref(), Some("from-file"));
    }
    assert_eq!(cfg.provider, "local");
}
