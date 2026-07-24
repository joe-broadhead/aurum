use aurum_core::cancel::CancelFlag;
use aurum_core::error::{ProviderError, TranscriptionError};
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use aurum_core::window::{PartialClock, PartialWindowPolicy};
use std::path::PathBuf;

#[tokio::test]
async fn cancel_before_decode_errors() {
    let flag = CancelFlag::new();
    flag.cancel();
    let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cancel-test"))
        .with_progress(false)
        .with_local_only(true);
    // Even without a model, cancel is checked first when samples valid...
    // Empty pcm fails first — use non-empty.
    let samples = vec![0.0f32; 1600];
    let opts = TranscriptionOptions {
        model: "tiny-q5_1".into(),
        language: "en".into(),
        timestamps: false,
        cancel: Some(flag),
    };
    let err = provider.transcribe_pcm(&samples, &opts).await.unwrap_err();
    assert!(
        matches!(
            err,
            TranscriptionError::Provider(ProviderError::Cancelled) | TranscriptionError::User(_) // local_only missing model if cancel order differs
        ),
        "got {err}"
    );
    // Prefer cancelled when model missing after cancel check — we check cancel first.
    assert!(matches!(
        err,
        TranscriptionError::Provider(ProviderError::Cancelled)
    ));
}

#[test]
fn partial_clock_take_slice() {
    let policy = PartialWindowPolicy {
        min_partial_samples: 100,
        window_samples: 50,
        interval_nanos: 0,
        min_rms_bits: 0,
    };
    let mut clock = PartialClock::new(policy);
    let samples: Vec<f32> = (0..200).map(|i| i as f32 * 0.01).collect();
    let slice = clock.take_partial_slice(&samples).expect("ready");
    assert_eq!(slice.len(), 50);
}
