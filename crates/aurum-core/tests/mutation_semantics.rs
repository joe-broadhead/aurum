//! Semantic mutation guards for trust-critical pure logic (JOE-1886).
//!
//! Always-on kill list for bounds / DTO validation / digest pins. cargo-mutants
//! exercises a broader surface; these tests fail immediately if fail-closed
//! behavior is weakened.

use aurum_core::domain::{FiniteDurationSecs, ModelId, SampleRateHz};
use aurum_core::dto::SttResultDto;
use aurum_core::model::pinned_sha256;
use aurum_core::providers::{Segment, TranscriptionResult};

#[test]
fn sample_rate_zero_is_rejected() {
    assert!(SampleRateHz::try_new(0).is_err());
    assert!(SampleRateHz::try_new(16_000).is_ok());
}

#[test]
fn duration_nan_and_negative_rejected() {
    assert!(FiniteDurationSecs::try_new(f64::NAN).is_err());
    assert!(FiniteDurationSecs::try_new(-0.1).is_err());
    assert!(FiniteDurationSecs::try_new(0.0).is_ok());
}

#[test]
fn model_id_empty_and_control_rejected() {
    assert!(ModelId::try_new("").is_err());
    assert!(ModelId::try_new("  ").is_err());
    assert!(ModelId::try_new("a\nb").is_err());
    assert!(ModelId::try_new("tiny").is_ok());
}

#[test]
fn segment_bounds_fail_closed() {
    assert!(Segment::try_new(0.0, 1.0, "ok").is_ok());
    assert!(Segment::try_new(f64::NAN, 1.0, "x").is_err());
    assert!(Segment::try_new(2.0, 1.0, "x").is_err());
    assert!(Segment::try_new(-1.0, 0.0, "x").is_err());
}

#[test]
fn try_from_dto_cannot_skip_segment_validation() {
    let mut dto = SttResultDto::from_result(&TranscriptionResult::local(
        "x".into(),
        vec![Segment::try_new(0.0, 1.0, "x").unwrap()],
        None,
        "m".into(),
        1.0,
    ));
    dto.segments = vec![Segment::from_parts_unchecked(
        f64::NAN,
        1.0,
        "x".to_string(),
    )];
    assert!(
        TranscriptionResult::try_from_dto(&dto).is_err(),
        "DTO with NaN segment must not convert to domain"
    );
}

#[test]
fn catalogue_models_have_pinned_sha256() {
    // JOE-1645 / digest integrity — pin lookup must stay fail-closed for catalogue files.
    let names = [
        "ggml-tiny-q5_1.bin",
        "ggml-base-q5_1.bin",
        "ggml-small-q5_1.bin",
    ];
    for n in names {
        let pin = pinned_sha256(n);
        assert!(pin.is_some(), "missing pin for {n}");
        let p = pin.unwrap();
        assert_eq!(p.len(), 64, "sha256 hex length for {n}");
        assert!(p.chars().all(|c| c.is_ascii_hexdigit()), "hex for {n}");
    }
}
