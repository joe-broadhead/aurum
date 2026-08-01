# Native embeds (`aurum-ffi`)

C ABI façade over `aurum-core` for Swift, Kotlin, C, C#, and other hosts that
should not shell out to the CLI.

**On-device only** in this crate: no OpenRouter, no mic capture, no streaming loop.
Partials and hold-to-talk UX stay in the host.

| | |
|--|--|
| Header | [`crates/aurum-ffi/include/aurum.h`](https://github.com/joe-broadhead/aurum/blob/master/crates/aurum-ffi/include/aurum.h) |
| Crate | `aurum-ffi` (workspace; not required for CLI users) |
| Sample rate | **16 000 Hz** mono `float32` |
| ABI | `AURUM_ABI_VERSION` **2** (jobs, capabilities, doctor, TTS jobs; v1 blocking surface kept) |

## Build & install (JOE-1785)

```bash
cargo build -p aurum-ffi --release
# → target/release/libaurum_ffi.{a,dylib,so} or aurum_ffi.dll
# header: crates/aurum-ffi/include/aurum.h
```

### Downstream C11 / C++17 (CI-qualified on Linux/macOS)

```bash
INC=crates/aurum-ffi/include
LIB=target/release

# C11 job smoke
cc -std=c11 -I "$INC" crates/aurum-ffi/examples/job_cleanup.c \
  -L "$LIB" -laurum_ffi -lpthread -ldl -lm -o /tmp/aurum_job_cleanup

# C++17 RAII engine
c++ -std=c++17 -I "$INC" crates/aurum-ffi/examples/engine_raii.cpp \
  -L "$LIB" -laurum_ffi -lpthread -ldl -lm -o /tmp/aurum_engine_raii
```

Windows MSVC host-link of these staticlib examples is deferred; Unix CI runs the
lifecycle smoke. ABI constant/layout guards live in `crates/aurum-ffi/tests/abi_layout.rs`.

### Engine vs process pools

Rust library hosts should prefer `aurum_core::AurumEngine` (engine-local STT/TTS
pools). The FFI `AurumEngine` handle currently uses process-global whisper/TTS
residency on destroy/shutdown paths for Metal safety — see header lifetime notes
and JOE-1795 for full migration.

Rust hosts can use the same façade without C:

```rust
use aurum_ffi::{cleanup_rules, CleanupStyle, Engine, EngineConfig, TranscribeOpts};

let engine = Engine::new(EngineConfig {
    cache_dir: cache_dir.into(),
    local_only: true,
    progress_logging: false,
})?;
if engine.is_model_ready("tiny-q5_1") {
    engine.preload("tiny-q5_1")?;
}
let t = engine.transcribe_pcm(
    &pcm, // mono f32 @ 16 kHz
    &TranscribeOpts {
        model: "tiny-q5_1".into(),
        language: "en".into(),
        timestamps: false,
    },
)?;
let cleaned = cleanup_rules(&t.text, CleanupStyle::Clean)?;
drop(engine);
aurum_ffi::shutdown(); // before process exit (Metal-safe)
```

## C surface (summary)

| Call | Role |
|------|------|
| `aurum_engine_create` / `destroy` | Handle + cache dir; `local_only` default recommended |
| `aurum_engine_preload` | Download (unless local_only) + warm context |
| `aurum_engine_is_model_ready` | Cache probe (read-only) |
| `aurum_engine_transcribe_pcm` | Decode |
| `aurum_engine_cancel` | Cooperative cancel (other thread OK) |
| `aurum_cleanup_rules` | On-device text cleanup (no engine required) |
| `aurum_shutdown` / `aurum_shutdown_ex` | Drain exclusive ops; clear whisper cache only when idle (`BUSY` if still active) |

Zero-initialize config/opts structs (`reserved` must be `0`).

## Threading & jobs (ABI v2 / JOE-1577)

- **Blocking:** one exclusive op (`preload` / `transcribe_pcm`) **per engine** at a time.
- **Jobs:** `aurum_job_start_*` returns immediately; poll/wait/cancel/free. Prefer jobs
  from event-loop threads so you never nest Tokio.
- **Distinct engines** may run concurrently; `aurum_engine_shutdown` drains one engine only.
- Process `aurum_shutdown_ex` closes global admission and clears whisper caches only when idle.
- Ownership: **engine → jobs → results** (transcript / string / audio). Result take is once.
- `aurum_engine_last_error` is valid only until the next call on the **same thread** — copy it.

### Job API (summary)

| Call | Role |
|------|------|
| `aurum_job_start_preload` / `_transcribe` / `_cleanup` / `_tts` | Nonblocking start |
| `aurum_job_poll` | State + progress (0–100) |
| `aurum_job_wait` | Block until terminal or timeout (no auto-cancel) |
| `aurum_job_cancel` | Cooperative cancel (other thread OK) |
| `aurum_job_take_*` | Transfer result exactly once |
| `aurum_job_free` | Drop handle (cancels if still running) |

### Capabilities & doctor

```c
AurumCapabilities caps = {0};
caps.struct_version = 1;
aurum_capabilities(&caps);   /* has_stt/tts/jobs/doctor, sample rate */

char *json = NULL;
aurum_doctor_json(&json);    /* free with aurum_string_free */
```

CLI: `aurum doctor` / `aurum doctor --json`.

## Host checklist

| Concern | Guidance |
|---------|----------|
| Resample | Host must deliver 16 kHz mono f32 |
| First run | `is_model_ready` → UX → `preload` |
| Offline | `local_only = 1` |
| Cancel | `aurum_engine_cancel` mid-hold |
| Cleanup | Separate stage after ASR (`cleanup_rules`) |
| Exit | Free job results → `aurum_engine_shutdown` → `destroy` engines → `aurum_shutdown_ex` (jobs count toward drain) |
| Partials | Host loop + rolling PCM window; see [Partials](partials.md) |

## Related

- [Library overview](overview.md)  
- [Rust integration](integration.md)  
- [Architecture](../development/architecture.md)  
