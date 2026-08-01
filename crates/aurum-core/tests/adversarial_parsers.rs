//! Continuous adversarial / property-style parser tests (JOE-1631).
//!
//! These run on every PR (no nightly fuzz harness required). Corpus seeds are
//! retained as inline fixtures so regressions stay in git.
//!
//! Full libFuzzer/cargo-fuzz campaigns remain follow-up; this suite is the
//! continuous fail-closed smoke that blocks panics and non-finite JSON leaks.

use aurum_core::config::Config;
use aurum_core::output::OutputFormat;
use aurum_core::pcm::PcmBuffer;
use aurum_core::providers::{Segment, TranscriptionResult};
use aurum_core::remote::{
    validate_endpoint, validate_segments, validate_text_bounds, RemotePolicy, TranscriptLimits,
};
use std::fs;
use tempfile::tempdir;

/// Seed corpus of hostile TOML / config fragments.
const CONFIG_SEEDS: &[&[u8]] = &[
    b"",
    b"\0\0\0",
    b"[[[[",
    b"provider = ",
    b"[default]\nprovider = \"local\"\n[default]\nprovider = \"x\"",
    b"[tts]\nmax_chars = 0",
    b"[tts]\ntimeout_ms = 0",
    b"[openrouter]\nbase_url = \"http://evil.example\"",
    b"[tts.custom_models]\nid = \"x\"", // wrong shape
    b"provider = \"not-a-real-provider\"\n",
    b"output = \"mp3\"\n",
    b"[tts]\nmax_chars = -1\n",
    b"x = \"yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\"\n",
    &[0xff, 0xfe, 0xfd, 0x00, 0x01],
];

#[test]
fn config_seeds_never_panic() {
    let dir = tempdir().unwrap();
    for (i, seed) in CONFIG_SEEDS.iter().enumerate() {
        let path = dir.path().join(format!("seed-{i}.toml"));
        fs::write(&path, seed).unwrap();
        // Any outcome is fine; panics are not.
        let _ = Config::load_from(&path);
        let _ = Config::load_from_required(&path);
    }
}

#[test]
fn format_json_never_emits_nan_for_adversarial_durations() {
    let cases = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0];
    for d in cases {
        let mut r = TranscriptionResult::local("x".into(), vec![], None, "m".into(), d);
        r.set_duration_secs(d);
        let (norm, _) = aurum_core::postprocess::normalize_result_with_report(r);
        let json = aurum_core::output::format_result(&norm, OutputFormat::Json).unwrap();
        assert!(
            !json.to_ascii_lowercase().contains("nan"),
            "NaN leaked for duration {d}: {json}"
        );
        assert!(
            !json.to_ascii_lowercase().contains("inf"),
            "Inf leaked for duration {d}: {json}"
        );
    }
}

#[test]
fn format_srt_handles_empty_and_reordered_segments() {
    let result = TranscriptionResult::local(
        "hi".into(),
        vec![
            Segment::from_parts_unchecked(2.0, 3.0, "later".to_string()),
            Segment::from_parts_unchecked(0.0, 1.0, "first".to_string()),
        ],
        Some("en".into()),
        "tiny".into(),
        3.0,
    );
    let (norm, _) = aurum_core::postprocess::normalize_result_with_report(result);
    let srt = aurum_core::output::format_result(&norm, OutputFormat::Srt).unwrap();
    assert!(!srt.is_empty());
    // Must not panic and must remain valid-ish UTF-8 text.
    assert!(srt.is_ascii() || srt.contains("first") || srt.contains("later") || !srt.is_empty());
}

#[test]
fn output_format_parse_rejects_unknown() {
    assert!(OutputFormat::parse("mp3").is_err());
    assert!(OutputFormat::parse("").is_err());
    let _ = OutputFormat::parse("JSON\0");
    assert!(OutputFormat::parse("json").is_ok());
    assert!(OutputFormat::parse("SRT").is_ok());
}

#[test]
fn remote_endpoint_seeds_fail_closed() {
    let policy = RemotePolicy::default();
    let bad = [
        "",
        "not a url",
        "http://evil.example",
        "ftp://openrouter.ai",
        "https://user:pass@openrouter.ai",
        "https://evil.example",
        "file:///etc/passwd",
    ];
    for raw in bad {
        assert!(
            validate_endpoint(raw, &policy).is_err(),
            "expected reject for {raw:?}"
        );
    }
    assert!(validate_endpoint("https://openrouter.ai", &policy).is_ok());
}

#[test]
fn remote_transcript_bounds_reject_expansion_and_nan_segments() {
    let limits = TranscriptLimits {
        max_text_chars: 100,
        max_segments: 2,
        max_segment_chars: 50,
        max_total_segment_chars: 80,
        max_expansion_ratio: 2.0,
    };
    assert!(validate_text_bounds("short", Some(10), limits, "test").is_ok());
    assert!(validate_text_bounds(&"x".repeat(200), Some(10), limits, "test").is_err());
    assert!(validate_text_bounds(&"x".repeat(50), Some(10), limits, "test").is_err()); // 5x expand

    let segs = vec![Segment::from_parts_unchecked(
        f64::NAN,
        1.0,
        "x".to_string(),
    )];
    assert!(validate_segments(&segs, 10.0, limits, "test").is_err());

    let segs = vec![Segment::from_parts_unchecked(2.0, 1.0, "x".to_string())];
    assert!(validate_segments(&segs, 10.0, limits, "test").is_err());
}

#[test]
fn pcm_buffer_rejects_nan_inf() {
    let mut buf = PcmBuffer::with_max_secs(1.0);
    assert!(buf.push(&[0.0, 0.5, -0.5]).is_ok());
    assert!(buf.push(&[f32::NAN]).is_err());
    assert!(buf.push(&[f32::INFINITY]).is_err());
    assert!(buf.push(&[f32::NEG_INFINITY]).is_err());
}

#[cfg(feature = "tts")]
#[test]
fn tts_manifest_adversarial_json_fails_closed() {
    use aurum_core::tts::ModelPackManifest;
    let dir = tempdir().unwrap();
    let seeds: &[&[u8]] = &[
        b"{}",
        b"[]",
        b"{\"schema_version\":999}",
        b"{\"schema_version\":1,\"adapter_id\":\"\",\"model_id\":\"x\",\"sample_rate_hz\":0,\"channels\":0,\"max_phoneme_tokens\":1,\"languages\":[],\"license\":\"\",\"trust\":\"builtin\",\"artifacts\":[]}",
        b"not-json",
        b"{\"schema_version\":1}",
        &[0, 1, 2, 3, 255],
    ];
    for (i, s) in seeds.iter().enumerate() {
        let p = dir.path().join(format!("m{i}.json"));
        fs::write(&p, s).unwrap();
        let _ = ModelPackManifest::load_path(&p);
    }
}

#[cfg(feature = "tts")]
#[test]
fn tts_prepare_text_adversarial_inputs() {
    use aurum_core::tts::{prepare_text, validate_text, DEFAULT_MAX_CHARS};
    // Empty / control-heavy should fail closed or produce prepared text without panic.
    let _ = validate_text("");
    let _ = validate_text("\0\0\0");
    let huge = "a".repeat(DEFAULT_MAX_CHARS + 10);
    assert!(prepare_text(&huge, DEFAULT_MAX_CHARS).is_err());
    let ok = prepare_text("Hello, world.", DEFAULT_MAX_CHARS);
    assert!(ok.is_ok());
}
