# Partials & cancel

Aurum does **not** run a background streaming whisper loop and does **not** own
the microphone. Dictation hosts should prefer **`PartialSession`** (JOE-1605)
for stable/unstable text, monotonic revisions, and supersession cancel.

## Recommended: `PartialSession`

```rust
use aurum_core::partial::PartialSession;
use aurum_core::providers::{LocalWhisperProvider, TranscriptionOptions};
use std::path::PathBuf;

# async fn demo() -> aurum_core::Result<()> {
let provider = LocalWhisperProvider::new(PathBuf::from("/tmp/aurum-cache"))
    .with_progress(false)
    .with_local_only(true);

let mut session = PartialSession::dictation();
// on each mic chunk:
// session.push(&chunk)?;
// if let Some(cancel) = session.begin_partial() {
//     let window = session.partial_window_samples();
//     let opts = TranscriptionOptions {
//         model: "tiny-q5_1".into(),
//         language: "en".into(),
//         timestamps: false,
//         cancel: Some(cancel),
//     };
//     match provider.transcribe_pcm(window.as_slice(), &opts).await {
//         Ok(r) => { let u = session.on_decode_result(&r.text, None); /* UI */ }
//         Err(_) => session.end_partial(),
//     }
// }
// on release:
// let final_r = provider.transcribe_pcm(session.buffer().samples().as_slice(), &opts).await?;
// let done = session.finalize(&final_r.text, None);
aurum_core::providers::local::clear_context_cache();
# Ok(())
# }
```

`PartialWindowPolicy` / `PartialClock` remain available for simple loops (used
internally by `PartialSession` for energy/interval gates).

## Defaults (dictation-oriented)

| Parameter | Default |
|-----------|---------|
| Min audio before partial | ~1 s |
| Rolling window | ~15 s |
| Interval | ~1.2 s |
| Energy gate | RMS on the **decode window** |
| Max inflight partials | 1 (supersede/cancel extras) |

Tune via `PartialSessionConfig` / `PartialWindowPolicy`.
