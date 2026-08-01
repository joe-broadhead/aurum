//! Integration-style unit coverage for formatters and config (no model download).

use aurum_core::config::Config;
use aurum_core::output::{format_result, OutputFormat};
use aurum_core::providers::{Segment, TranscriptionResult};
use std::fs;
use tempfile::tempdir;

fn sample() -> TranscriptionResult {
    TranscriptionResult::local(
        "One two three.".into(),
        vec![Segment::from_parts_unchecked(
            0.0,
            2.0,
            "One two three.".to_string(),
        )],
        Some("en".into()),
        "tiny".into(),
        2.0,
    )
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
fn json_marks_local_timestamps_reliable() {
    let s = format_result(&sample(), OutputFormat::Json).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["timestamps_reliable"], true);
    assert_eq!(v["backend_kind"], "asr");
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

    let cfg = Config::load_from(&path).unwrap();
    if std::env::var_os("OPENROUTER_API_KEY").is_none() {
        assert_eq!(
            cfg.openrouter_api_key.as_ref().map(|s| s.expose()),
            Some("from-file")
        );
    }
    assert_eq!(cfg.provider, "local");
}
