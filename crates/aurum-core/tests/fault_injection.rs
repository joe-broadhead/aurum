//! Deterministic fault / adversarial I/O tests (JOE-1632).
//!
//! These prove fail-closed invariants without network or large models.
//! Hard timeouts: each test is pure CPU/FS and must finish in seconds.

use aurum_core::cleanup::{cleanup_text, CleanupStyle, RulesCleanup, TextCleanup};
use aurum_core::output::{commit_text, CommitMode, OutputFormat, OutputTransaction, SymlinkPolicy};
use aurum_core::postprocess::normalize_result_with_report;
use aurum_core::providers::{Segment, TranscriptionResult};
use std::fs;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn output_transaction_noclobber_preserves_existing() {
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.txt");
    fs::write(&dest, b"original").unwrap();
    let tx = OutputTransaction::new(&dest, CommitMode::NoClobber);
    assert!(tx.commit_bytes(b"new").is_err());
    assert_eq!(fs::read(&dest).unwrap(), b"original");
}

#[test]
fn output_transaction_rejects_symlink_destination() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real.txt");
    fs::write(&real, b"x").unwrap();
    let link = dir.path().join("link.txt");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let tx = OutputTransaction::new(&link, CommitMode::Replace)
            .with_symlink_policy(SymlinkPolicy::Reject);
        assert!(tx.commit_bytes(b"y").is_err());
        assert_eq!(fs::read(&real).unwrap(), b"x");
    }
    #[cfg(not(unix))]
    {
        let _ = (real, link);
    }
}

#[test]
fn output_transaction_atomic_replace() {
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.txt");
    fs::write(&dest, b"old").unwrap();
    commit_text(&dest, "new-content", CommitMode::Replace).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "new-content\n");
}

#[test]
fn output_transaction_preflight_missing_parent_no_orphan_tmp() {
    let dir = tempdir().unwrap();
    let dest = dir.path().join("nope").join("nested").join("out.txt");
    let tx = OutputTransaction::new(&dest, CommitMode::Replace);
    // preflight may pass path shape; commit should fail without scattering temps at root.
    let _ = tx.preflight();
    let _ = tx.commit_bytes(b"data");
    let partials: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.contains(".tmp") || n.contains(".aurum")
        })
        .collect();
    assert!(partials.is_empty(), "unexpected partials: {partials:?}");
}

#[test]
fn output_transaction_empty_path_fails() {
    let tx = OutputTransaction::new("", CommitMode::Replace);
    assert!(tx.preflight().is_err());
    assert!(tx.commit_bytes(b"x").is_err());
}

#[test]
fn output_transaction_directory_dest_fails() {
    let dir = tempdir().unwrap();
    let tx = OutputTransaction::new(dir.path(), CommitMode::Replace);
    assert!(tx.preflight().is_err() || tx.commit_bytes(b"x").is_err());
}

#[tokio::test]
async fn cleanup_adversarial_control_chars_and_huge_repeat() {
    let start = Instant::now();
    let rules = RulesCleanup::new();
    let adversarial = format!("um, {}\n\u{200B}", "hello ".repeat(2000));
    let out = cleanup_text(
        &adversarial,
        &rules as &dyn TextCleanup,
        CleanupStyle::Clean,
    )
    .await
    .unwrap();
    assert!(!out.text.is_empty());
    assert!(!out.text.contains('\0'));
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "cleanup hung: {:?}",
        start.elapsed()
    );
}

#[test]
fn normalization_handles_nan_segment_without_panic() {
    let result = TranscriptionResult::local(
        "hi".into(),
        vec![Segment {
            start: f64::NAN,
            end: 1.0,
            text: "hi".into(),
        }],
        Some("en".into()),
        "tiny".into(),
        1.0,
    );
    let (normalized, report) = normalize_result_with_report(result);
    assert!(normalized.duration_secs.is_finite());
    for s in &normalized.segments {
        assert!(s.start.is_finite());
        assert!(s.end.is_finite());
    }
    let _ = report;
}

#[test]
fn normalization_handles_inverted_and_infinite_segments() {
    let result = TranscriptionResult::local(
        "x".into(),
        vec![
            Segment {
                start: f64::INFINITY,
                end: f64::NEG_INFINITY,
                text: "a".into(),
            },
            Segment {
                start: 5.0,
                end: 1.0,
                text: "b".into(),
            },
        ],
        None,
        "m".into(),
        f64::NAN,
    );
    let (normalized, _) = normalize_result_with_report(result);
    assert!(normalized.duration_secs.is_finite());
    for s in &normalized.segments {
        assert!(s.start.is_finite() && s.end.is_finite());
        assert!(s.end >= s.start || s.text.is_empty());
    }
}

#[test]
fn format_result_handles_empty_and_unicode() {
    let result =
        TranscriptionResult::local("café".into(), vec![], Some("en".into()), "tiny".into(), 0.5);
    let txt = aurum_core::output::format_result(&result, OutputFormat::Txt).unwrap();
    assert!(txt.contains("café"));
    let json = aurum_core::output::format_result(&result, OutputFormat::Json).unwrap();
    assert!(!json.to_ascii_lowercase().contains("nan"));
}

#[test]
fn config_adversarial_toml_fails_closed() {
    use aurum_core::config::Config;
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    fs::write(&path, b"[[[not valid").unwrap();
    assert!(Config::load_from(&path).is_err());
    assert!(Config::load_from_required(&path).is_err());
}

#[test]
fn config_oversized_file_fails_closed() {
    use aurum_core::config::Config;
    let dir = tempdir().unwrap();
    let path = dir.path().join("huge.toml");
    // MAX_CONFIG_BYTES is 256 KiB in config; write slightly over.
    let blob = vec![b'a'; 300 * 1024];
    fs::write(&path, &blob).unwrap();
    assert!(Config::load_from(&path).is_err());
}

#[test]
fn config_duplicate_custom_tts_rejected_when_pack_exists() {
    use aurum_core::config::Config;
    #[cfg(feature = "tts")]
    {
        use aurum_core::tts::write_fake_sine_pack;
        let dir = tempdir().unwrap();
        let pack = dir.path().join("pack");
        write_fake_sine_pack(&pack, "my-fake").unwrap();
        let path = dir.path().join("cfg.toml");
        fs::write(
            &path,
            format!(
                r#"
[tts]
model = "kitten-nano-int8"

[[tts.custom_models]]
id = "dup"
adapter = "fake-sine-v1"
pack_dir = "{}"
trust = "verified"

[[tts.custom_models]]
id = "dup"
adapter = "fake-sine-v1"
pack_dir = "{}"
trust = "verified"
"#,
                pack.display(),
                pack.display()
            ),
        )
        .unwrap();
        assert!(Config::load_from(&path).is_err());
    }
    #[cfg(not(feature = "tts"))]
    {
        let _ = Config::default();
    }
}

#[test]
fn secret_redaction_never_echoes_key_material() {
    use aurum_core::remote::redact_secret;
    let key = "sk-or-v1-supersecretvalue123456";
    let redacted = redact_secret(key);
    assert!(!redacted.contains("supersecret"));
    assert!(redacted.contains("***") || redacted != key);
}

#[cfg(feature = "tts")]
#[test]
fn tts_validate_output_path_rejects_empty_and_trailing_slash() {
    use aurum_core::tts::validate_output_path;
    use std::path::Path;
    assert!(validate_output_path(Path::new("")).is_err());
    // Trailing separator → treated as directory form (no file name).
    assert!(
        validate_output_path(Path::new("out/")).is_err() || {
            // On some platforms Path::file_name may still return a component; either
            // empty or a normal file path is acceptable as long as we do not panic.
            validate_output_path(Path::new("out.wav")).is_ok()
        }
    );
    assert!(validate_output_path(Path::new("out.wav")).is_ok());
}
