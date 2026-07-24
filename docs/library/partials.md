# Partials & cancel

Aurum does **not** run a background streaming whisper loop. Dictation hosts can
still show interim text by:

1. Accumulating mic PCM in `PcmBuffer` (or your own `Vec<f32>`)
2. Using `PartialWindowPolicy` / `PartialClock`
3. Calling `transcribe_pcm` on a rolling slice when the clock is ready
4. Calling `transcribe_pcm` on the full buffer at finalize
5. Using `CancelFlag` if the user cancels mid-hold

## Example

```rust
use aurum_core::cancel::CancelFlag;
use aurum_core::pcm::PcmBuffer;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use aurum_core::window::{PartialClock, PartialWindowPolicy};
use std::path::PathBuf;

# async fn demo() -> aurum_core::Result<()> {
let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
    .with_progress(false)
    .with_local_only(true);
provider.preload("tiny-q5_1").await?;

let mut buf = PcmBuffer::dictation();
let mut clock = PartialClock::new(PartialWindowPolicy::dictation());
let cancel = CancelFlag::new();

// on each mic chunk:
// buf.push(&chunk)?;
// if let Some(slice) = clock.take_partial_slice(buf.samples()) {
//     let partial = provider.transcribe_pcm(slice, &TranscriptionOptions {
//         model: "tiny-q5_1".into(),
//         language: "en".into(),
//         timestamps: false,
//         cancel: Some(cancel.clone()),
//     }).await?;
//     // show partial.text in UI
// }

// on cancel:
// cancel.cancel();

// on release / finalize:
let final_opts = TranscriptionOptions {
    model: "tiny-q5_1".into(),
    language: "en".into(),
    timestamps: false,
    cancel: Some(cancel.clone()),
};
let _final = provider.transcribe_pcm(buf.samples(), &final_opts).await?;
aurum_core::providers::local::clear_context_cache();
# Ok(())
# }
```

## Defaults (dictation-oriented)

| Parameter | Default |
|-----------|---------|
| Min audio before partial | ~1 s |
| Rolling window | ~15 s |
| Interval | ~1.2 s |
| Energy gate | RMS on the **decode window** |

Tune via `PartialWindowPolicy` / `with_min_rms`.
