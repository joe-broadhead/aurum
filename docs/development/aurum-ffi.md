# aurum-ffi — production design specification

**Status:** Draft for implementation (not shipped)  
**Product line:** Aurum — *Audio in. Text out. On-device by default.*  
**Depends on:** `aurum-core` (Rust), whisper.cpp via `whisper-rs`  
**Non-goal of v1:** Swift package polish, OpenRouter across FFI, streaming decoder loop  

This document is the source of truth for a **stable, host-facing foreign interface** over Aurum’s on-device engine. It is rooted in first principles: small surface, fail closed, honesty about capabilities, and clean separation between **engine** and **app policy**.

---

## 0. First principles

### 0.1 What problem FFI solves

Native apps (Swift, Kotlin, C#, C) need on-device STT without:

- shelling out to a CLI,
- embedding a second copy of model-download / ggml policy,
- taking a hard dependency on Tokio, async traits, or Cargo in the app build.

They already own:

- microphone capture and resampling,
- UI lifecycle and cancellation,
- “when to show a partial,”
- where files and caches live.

**Aurum’s job at the boundary:** accept PCM (or a simple file path later), run local ASR, optionally apply **on-device** text cleanup, return structured text + honesty metadata.

### 0.2 Design laws

| # | Law | Consequence |
|---|-----|-------------|
| 1 | **On-device by default** | v1 FFI has **no network APIs**. Downloads are explicit and local-cache scoped. |
| 2 | **Narrow stable surface** | Freeze a *façade*, not all of `aurum-core`. |
| 3 | **Host owns policy** | Partials, VAD, push-to-talk UX live in the app. Engine is callable and cancelable. |
| 4 | **Fail closed** | Bad sample rate, missing model (if local-only), cancel, OOM → typed errors. No silent empty success. |
| 5 | **Honesty fields travel** | `timestamps_reliable`, backend class, cleanup style are first-class in results. |
| 6 | **One ownership model** | Handles own engine state; strings/buffers returned to host are either copied out or freed by documented `aurum_*_free`. |
| 7 | **Thread rules are part of the API** | Documented, tested, not “undefined but works on my Mac.” |
| 8 | **ABI versioning ≠ crate versioning** | `aurum_abi_version()` is independent; breaking FFI is a major ABI bump. |
| 9 | **Clean code at the boundary** | No logic in UniFFI scaffolding; pure translate → call core → translate. |
| 10 | **Test the contract** | C header smoke + Rust tests through the façade; Swift is integration, not the unit of correctness. |

### 0.3 Non-principles (explicit rejects)

- “Expose every Rust type via UniFFI.”
- “Stable async callbacks for partials from inside whisper.”
- “Bundle ffmpeg in the FFI dylib for v1.”
- “Stable OpenRouter in the same ABI as local ASR.”
- “Share `WhisperContext` across processes.” (cache dir on disk is shared; contexts are process-local.)

---

## 1. Goals and non-goals

### 1.1 Goals (v1)

1. Ship a **loadable native library** (`libaurum_ffi`) with:
   - C ABI header `aurum.h` (source of truth for non-UniFFI hosts),
   - UniFFI bindings for Kotlin/Swift as **generated consumers** of the same façade.
2. Support dictation-class hosts:
   - `preload` model,
   - `transcribe_pcm`,
   - cooperative `cancel`,
   - **rules** cleanup (sync, pure).
3. Predictable lifecycle: create → use → destroy; destroy clears Metal/ggml process cache when last handle dies (or explicit shutdown).
4. Production packaging path: static/dynamic lib + headers; optional XCFramework *later*.
5. SemVer for the **`aurum-ffi` crate** and a monotonic **ABI integer**.

### 1.2 Non-goals (v1)

| Non-goal | Rationale |
|----------|-----------|
| OpenRouter / any cloud | Keys, TLS, privacy, async; pollutes “on-device by default” |
| Built-in mic capture | Platform-specific; host responsibility |
| Continuous streaming whisper thread | Core is batch-on-slice; host loops |
| Diarization / translation / LLM rewrite | Product creep |
| Stable guarantee for full `aurum-core` Rust API | Core may break until 0.1/1.0 |
| Plugin backends over FFI | ABI and safety complexity |
| Guaranteed realtime RT priority | Best-effort cancel only |

### 1.3 Success metrics

- Swift/Kotlin host can hold-to-talk: preload once, PCM push in app, finalize `transcribe_pcm` &lt; UX budget on `tiny-q5_1` / `base-q5_1`.
- Cancel stops decode without process abort (Metal-safe teardown path exercised).
- Zero network in default configuration (enforce with test + optional `local_only`).
- Header + UniFFI stay in lockstep via single Rust façade module.

---

## 2. Architecture

### 2.1 Crate layout

```text
crates/
  aurum-core/          # existing engine (may churn)
  aurum-stt/           # CLI (unchanged role)
  aurum-ffi/           # NEW — stable façade + C + UniFFI
    src/
      lib.rs           # crate root, version constants
      facade.rs        # pure Rust API used by both C and UniFFI
      c_api.rs         # #[no_mangle] extern "C"
      error.rs         # FfiErrorCode + message mapping
      types.rs         # POD / owned mirror types
      runtime.rs       # Tokio + thread pool policy
      handle.rs        # EngineHandle internals
    include/
      aurum.h          # public C header (generated or hand-synced)
    uniffi/
      aurum.udl        # or proc-macro UniFFI
    tests/
      c_smoke.rs
      facade_pcm.rs
```

**Dependency direction:**

```text
aurum-stt ──► aurum-core
aurum-ffi ──► aurum-core
     ▲
     │ never reverse
```

CLI must not depend on FFI. Core must not depend on FFI.

### 2.2 Layering

```text
┌─────────────────────────────────────────┐
│  Host (Swift / Kotlin / C / C#)         │
│  mic · UI · partial policy · file I/O   │
└─────────────────┬───────────────────────┘
                  │ C ABI  or  UniFFI
┌─────────────────▼───────────────────────┐
│  aurum-ffi                              │
│  validate · map errors · own handles    │
│  block_on engine calls · copy strings   │
└─────────────────┬───────────────────────┘
                  │ Rust only
┌─────────────────▼───────────────────────┐
│  aurum-core                             │
│  LocalWhisperProvider · RulesCleanup    │
│  CancelFlag · model cache · postprocess │
└─────────────────────────────────────────┘
```

### 2.3 Single façade rule

All exports (C and UniFFI) call **only** `facade::*`.  
No `whisper-rs` types, no `async_trait` objects, no `TranscriptionProvider` dyn in the public FFI story.

```rust
// conceptual
pub struct Engine { /* … */ }

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, FfiError>;
    pub fn preload(&self, model: &str) -> Result<(), FfiError>;
    pub fn is_model_ready(&self, model: &str) -> bool;
    pub fn transcribe_pcm(&self, pcm: &[f32], opts: TranscribeOpts) -> Result<Transcript, FfiError>;
    pub fn cancel(&self);
    pub fn reset_cancel(&self);
    pub fn cleanup_rules(text: &str, style: CleanupStyle) -> Result<String, FfiError>; // associated or free fn
}
```

C and UniFFI are **projection layers**, not parallel implementations.

---

## 3. Public conceptual API (façade)

### 3.1 Constants

| Name | Value / rule |
|------|----------------|
| `AURUM_ABI_VERSION` | `u32`, start at `1` |
| `AURUM_SAMPLE_RATE` | `16000` (must match `WHISPER_SAMPLE_RATE`) |
| `AURUM_SAMPLE_FORMAT` | mono `f32` little-endian, range ≈ [-1, 1] |

### 3.2 Configuration (`EngineConfig`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `cache_dir` | path string | **required** in v1 | Host picks app support / cache dir |
| `local_only` | bool | `true` | If true, never download; missing model → error |
| `progress_logging` | bool | `false` | No callbacks in v1; optional stderr only if true |

**v1 omit:** config file discovery, OpenRouter keys, custom base URLs.

### 3.3 Transcribe options (`TranscribeOpts`)

| Field | Type | Default |
|-------|------|---------|
| `model` | string | `"base"` or document `"tiny-q5_1"` as recommended embed default |
| `language` | string | `"auto"` |
| `timestamps` | bool | `false` |
| `max_secs` | f64 optional | core limit applies if unset |

Cancel is **not** an option field on the C API: it is **handle-scoped** (see §5). One in-flight cancel flag per engine handle avoids races with “which call do I cancel?”

### 3.4 Transcript result (`Transcript`)

| Field | Type | Notes |
|-------|------|-------|
| `text` | string | Final text (cleanup not auto-applied unless opts say so — see §3.5) |
| `language` | string nullable | Detected or requested |
| `model` | string | Echo |
| `duration_secs` | f64 | Audio duration |
| `backend` | enum | v1: only `Asr` |
| `timestamps_reliable` | bool | v1: always `true` for local |
| `segments` | list of `{start_s, end_s, text}` | Empty if `timestamps == false` |
| `cleanup_style` | enum | `Raw` unless cleanup applied in-engine |

### 3.5 Cleanup

**Two ways (pick one in implementation; prefer A for clarity):**

**A — Separate call (recommended)**  
Host always gets raw ASR from `transcribe_pcm`, then:

```text
cleaned = cleanup_rules(text, style)
```

Pros: pure, sync, testable, no coupling to decode.  
Matches Zephyr-style “ASR then flow” without naming Zephyr.

**B — Optional flag on transcribe**  
`cleanup: Raw | Clean | …` applied inside façade after ASR.

Pros: one round-trip.  
Cons: harder to show “original vs cleaned”; mixes stages.

**Spec decision: A is v1 normative. B is a convenience overload only if zero-cost and clearly documented.**

Styles (mirror core):

| Enum | Behavior |
|------|----------|
| `Raw` | identity / trim |
| `Clean` | fillers, spacing |
| `Bullets` | structural |
| `Professional` | rules formalization |
| `Summary` | extractive rules |

**No OpenRouter cleanup in v1 FFI.**

### 3.6 Model management

| Op | Behavior |
|----|----------|
| `preload(model)` | Ensure ggml on disk (unless `local_only`) + warm `WhisperContext` |
| `is_model_ready(model)` | File present (+ optional pin OK); does not require warm context |
| `list_models` (optional v1.1) | Names + cached bool; nice for settings UI |

v1 minimum: `preload` + `is_model_ready`. Listing can wait.

---

## 4. Error model

### 4.1 Principles

- Every fallible C function returns `AurumStatus` (`i32`).
- Success is `0`.
- Negative or positive ranges are fine if **stable** and documented; prefer small positive enum.
- Thread-local or handle-scoped **last error message** for humans; code for machines.

### 4.2 Status codes (normative v1)

```c
typedef enum AurumStatus {
  AURUM_OK = 0,

  AURUM_ERR_INVALID_ARG = 1,      /* null ptr, empty model, wrong rate */
  AURUM_ERR_STATE = 2,            /* use after destroy, not created */
  AURUM_ERR_MODEL_NOT_READY = 3,  /* local_only miss or not preloaded when required */
  AURUM_ERR_MODEL_DOWNLOAD = 4,   /* only if local_only=false */
  AURUM_ERR_INFERENCE = 5,        /* whisper failed */
  AURUM_ERR_CANCELLED = 6,        /* cooperative cancel */
  AURUM_ERR_AUDIO = 7,            /* empty pcm, too long, bad format */
  AURUM_ERR_INTERNAL = 8,         /* bug / invariant */
  AURUM_ERR_UNSUPPORTED = 9,      /* future: cloud, etc. */
  AURUM_ERR_NO_MEMORY = 10,
} AurumStatus;
```

Map from `aurum_core::TranscriptionError` taxonomy:

| Core | FFI |
|------|-----|
| User (bad path, bad model name, empty pcm) | `INVALID_ARG` / `AUDIO` |
| Environment (IO, ffmpeg if file API added) | `AUDIO` or new `IO` in v1.1 |
| Provider (download, inference) | `MODEL_*` / `INFERENCE` |
| Cancel | `CANCELLED` |
| Internal | `INTERNAL` |

### 4.3 Messages

- UTF-8, no secrets, actionable short text (reuse core wording where possible).
- C: `const char* aurum_last_error(const AurumEngine*)` or global if engine null during create failure.
- Prefer **per-handle** last error to avoid cross-talk between engines on multiple threads.

---

## 5. Concurrency and lifecycle

### 5.1 Handle model

```text
AurumEngine*  →  Arc<EngineInner>
EngineInner   →  LocalWhisperProvider, CancelFlag, Mutex/RwLock as needed,
                 last_error: Mutex<String>,
                 runtime: handle to shared or owned Tokio runtime
```

### 5.2 Threading rules (normative)

| Rule | Detail |
|------|--------|
| **T1** | `aurum_engine_create` / `destroy` are safe from any thread; destroy waits for in-flight work or fails with `STATE` if busy — **choose wait** for safety. |
| **T2** | At most **one** `transcribe_pcm` **per handle** at a time. Second call → `STATE` or internal queue; v1 = **reject concurrent** (`STATE`). |
| **T3** | `cancel` is wait-free / lock-free w.r.t. flag set; safe during transcribe from another thread. |
| **T4** | `cleanup_rules` is pure and reentrant (no handle required, or handle-optional). |
| **T5** | `preload` may run concurrent with cleanup; not concurrent with transcribe on same handle (v1 serialize). |
| **T6** | Host must not free PCM buffer until `transcribe_pcm` returns. |

### 5.3 Runtime policy

Core APIs are async today. FFI options:

| Option | Pros | Cons |
|--------|------|------|
| **Owned multi-thread Tokio runtime per process (lazy static)** | Simple | Lifetime vs tests |
| **Runtime per engine** | Isolation | Heavier |
| **`block_on` on caller thread with current-thread runtime** | No global | Deadlocks if caller is runtime thread |

**Spec decision:** process-wide lazy `Runtime` inside `aurum-ffi` (multi-thread, 1–2 workers sufficient for v1 embeds), documented so hosts don’t nest. Provide `aurum_shutdown()` to drop runtime + `clear_context_cache()` for tests and clean process exit (Metal).

### 5.4 Destroy / Metal

On last engine destroy or `aurum_shutdown()`:

```text
clear_context_cache()  // existing core API
```

Document: hosts **must** call destroy/shutdown before process exit on macOS embeds.

### 5.5 Cancel semantics

1. Each engine has one `CancelFlag`.
2. `transcribe_pcm` attaches that flag to `TranscriptionOptions`.
3. `aurum_cancel` sets flag.
4. Completed call returns `CANCELLED` if aborted; partial text is **not** required in v1 (may be empty).
5. `aurum_reset_cancel` clears flag before next utterance (or auto-reset at start of each transcribe).

**Auto-reset at start of `transcribe_pcm`** is required so hosts don’t forget.

---

## 6. C ABI (normative sketch)

### 6.1 Header outline (`aurum.h`)

```c
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AURUM_ABI_VERSION 1
#define AURUM_SAMPLE_RATE 16000

typedef struct AurumEngine AurumEngine;
typedef struct AurumTranscript AurumTranscript;

typedef enum AurumStatus { /* §4.2 */ } AurumStatus;

typedef enum AurumCleanupStyle {
  AURUM_CLEANUP_RAW = 0,
  AURUM_CLEANUP_CLEAN = 1,
  AURUM_CLEANUP_BULLETS = 2,
  AURUM_CLEANUP_PROFESSIONAL = 3,
  AURUM_CLEANUP_SUMMARY = 4,
} AurumCleanupStyle;

typedef struct AurumEngineConfig {
  const char *cache_dir;     /* required, UTF-8 */
  uint8_t local_only;        /* 1 = true */
  uint8_t progress_logging;  /* 0 default */
  uint8_t reserved[6];
} AurumEngineConfig;

typedef struct AurumTranscribeOpts {
  const char *model;         /* required */
  const char *language;      /* nullable → "auto" */
  uint8_t timestamps;        /* 0/1 */
  uint8_t reserved[7];
} AurumTranscribeOpts;

typedef struct AurumSegment {
  double start_s;
  double end_s;
  const char *text;          /* owned by parent transcript */
} AurumSegment;

/* ---- version / process ---- */
uint32_t aurum_abi_version(void);
const char *aurum_version(void);          /* crate semver string */
void aurum_shutdown(void);               /* optional clean teardown */

/* ---- engine lifecycle ---- */
AurumStatus aurum_engine_create(const AurumEngineConfig *cfg, AurumEngine **out);
void aurum_engine_destroy(AurumEngine *engine);
const char *aurum_engine_last_error(const AurumEngine *engine);

/* ---- models ---- */
AurumStatus aurum_engine_preload(AurumEngine *engine, const char *model);
uint8_t aurum_engine_is_model_ready(AurumEngine *engine, const char *model);

/* ---- decode ---- */
AurumStatus aurum_engine_transcribe_pcm(
    AurumEngine *engine,
    const float *samples,
    size_t n_samples,
    const AurumTranscribeOpts *opts,
    AurumTranscript **out_transcript);

void aurum_engine_cancel(AurumEngine *engine);

/* ---- transcript accessors (owned by transcript) ---- */
const char *aurum_transcript_text(const AurumTranscript *t);
const char *aurum_transcript_language(const AurumTranscript *t); /* nullable */
const char *aurum_transcript_model(const AurumTranscript *t);
double aurum_transcript_duration_secs(const AurumTranscript *t);
uint8_t aurum_transcript_timestamps_reliable(const AurumTranscript *t);
size_t aurum_transcript_segment_count(const AurumTranscript *t);
AurumStatus aurum_transcript_segment(const AurumTranscript *t, size_t i, AurumSegment *out);
void aurum_transcript_free(AurumTranscript *t);

/* ---- cleanup (pure, no engine required) ---- */
AurumStatus aurum_cleanup_rules(
    const char *text,
    AurumCleanupStyle style,
    char **out_text);          /* out_text freed with aurum_string_free */
void aurum_string_free(char *s);

#ifdef __cplusplus
}
#endif
```

### 6.2 ABI details

- All `const char*` inputs: UTF-8, non-null unless documented nullable.
- Outputs ending in `*_free` / `destroy`: nullable-safe (no-op on NULL).
- Structs passed by pointer; `reserved` fields zeroed by host (`memset 0`).
- No bitfields.
- Library is MT-safe under §5 rules.

### 6.3 Linking

| Platform | Artifact |
|----------|----------|
| macOS | `libaurum_ffi.dylib` + optional static `.a` |
| iOS (later) | XCFramework static |
| Linux | `libaurum_ffi.so` |
| Windows | `aurum_ffi.dll` + `aurum_ffi.lib` |

Features: default = metal on macOS via core’s target deps.

---

## 7. UniFFI projection

### 7.1 Role

UniFFI generates Swift/Kotlin from the **same façade**. It must not invent behavior.

Prefer **UDL or proc-macro** on façade types:

```text
namespace aurum {
  Engine create_engine(EngineConfig cfg);
  string cleanup_rules(string text, CleanupStyle style);
};

interface Engine {
  void preload(string model);
  boolean is_model_ready(string model);
  Transcript transcribe_pcm(sequence<f32> samples, TranscribeOpts opts);
  void cancel();
};
```

### 7.2 UniFFI constraints → design choices

| UniFFI pain | Our choice |
|-------------|------------|
| Async | Façade is **sync** (block_on inside) |
| Borrowed buffers | Copy or require owned `sequence<f32>` (copy once at boundary is OK for dictation lengths) |
| Errors | `Record`/`Enum` error type mapped from `FfiError` |
| Callbacks | None in v1 |

### 7.3 Dual surface discipline

CI check:

1. C header smoke test links `c_api`.
2. UniFFI bindgen runs.
3. Optional: cbindgen generates header from Rust annotations → diff against `include/aurum.h` (or header is fully generated).

**Prefer cbindgen-from-`c_api` as source of truth** to avoid drift.

---

## 8. Memory and performance

### 8.1 PCM path

- Host resamples to 16 kHz mono f32.
- Boundary validates: `n_samples > 0`, duration ≤ core max, finite samples (reject NaN/Inf like core postprocess spirit).
- No internal ring buffer in FFI v1 — host uses its own (`PcmBuffer` stays Rust-only helper).

### 8.2 Copies

| Data | Policy |
|------|--------|
| PCM in | Borrow in C API; UniFFI may copy |
| Transcript strings | Allocated once on success; freed by host |
| Segments | Stored contiguously in transcript object |

### 8.3 Size targets (guidance)

| Utterance | Expectation |
|-----------|-------------|
| ≤ 60 s dictation | Primary design point |
| ≤ 15 s partial windows | Host loops; each call independent |
| Multi-hour files | Out of scope for PCM embeds; file API later |

### 8.4 Preload

Hosts **should** preload on first launch / settings screen.  
Document recommended models: `tiny-q5_1` (trial), `base-q5_1` (default embed).

---

## 9. Security and privacy

| Topic | v1 rule |
|-------|---------|
| Network | Only model download when `local_only=0`; still no telemetry |
| Keys | None in FFI |
| Paths | Host-supplied cache dir; no path traversal beyond normal FS |
| Logs | Off by default; no PCM logging ever |
| Side channels | Error strings don’t include full PCM |
| Supply chain | Same model pins/magic as core |

---

## 10. Clean code standards (implementation)

### 10.1 Module rules

- `c_api.rs`: only `extern "C"`, null checks, `catch_unwind` → `INTERNAL`, call façade.
- `facade.rs`: no `unsafe`, no raw pointers.
- `handle.rs`: all `Mutex`/`Atomic` strategy documented in one place.
- No `unwrap`/`expect` on host-facing paths.
- `catch_unwind` at C boundary so panics never unwind into Swift.

### 10.2 Mapping functions

```text
try_status(Result<T, FfiError>) -> AurumStatus
store_error(engine, &FfiError)
transcript_from_core(TranscriptionResult) -> Transcript
```

Keep mappers pure and unit-tested.

### 10.3 What not to duplicate

- Do not reimplement cleanup rules in FFI.
- Do not fork model catalogue — call core.
- Do not reimplement cancel; wrap `CancelFlag`.

---

## 11. Testing strategy

### 11.1 Rust (required)

| Test | Asserts |
|------|---------|
| façade PCM empty | `AUDIO` |
| façade wrong conceptual use | errors map stably |
| cancel mid-decode | `CANCELLED` (may need long audio / tiny model) |
| cleanup_rules clean | filler stripped (fixture strings) |
| local_only missing model | `MODEL_NOT_READY` |
| concurrent double transcribe | `STATE` |
| destroy null | no-op |
| abi version | equals constant |

### 11.2 C smoke (required in CI)

Small `tests/c_smoke.c` or `cc` crate:

1. create engine with temp cache  
2. optional preload if model present / ignore download in CI  
3. silence/synthetic PCM or skip inference under flag  
4. cleanup_rules on `"um, hello"`  
5. destroy + shutdown  

### 11.3 Integration (nightly / manual)

- macOS Metal preload + real `tiny-q5_1` + fixture wav decoded to PCM in test harness  
- Mirror core’s `AURUM_INTEGRATION=1` pattern  

### 11.4 Bindings (later)

- Swift package smoke on macOS runner  
- Not a v1 gate for landing the crate  

---

## 12. Packaging and versioning

### 12.1 Crate

```toml
[package]
name = "aurum-ffi"
version = "0.0.0"   # track workspace until 0.1.0 FFI beta
```

Features:

- `default = ["c-api"]`
- `uniffi` optional  
- `c-api` = cbindgen/header  

### 12.2 Version matrix

| Component | Versioning |
|-----------|------------|
| `AURUM_ABI_VERSION` | Integer; +1 on any breaking C/UniFFI change |
| `aurum-ffi` SemVer | Major bump on ABI break; minor on additive APIs |
| `aurum-core` | Can break internals freely behind façade |

### 12.3 Release artifacts (future)

- GitHub Release attaches `libaurum_ffi` per target (separate job from CLI)  
- Or `cargo-c` / `cargo-xcframework` once iOS is real  

v1 can ship **source crate only** (build from source in app CI) before binary libs.

---

## 13. Host integration cookbook

### 13.1 Hold-to-talk (canonical)

```text
onAppStart:
  engine = create(cache_dir, local_only=true)
  if !is_model_ready("tiny-q5_1"): show download UX; set local_only=false once; preload
  else preload("tiny-q5_1")

onHoldStart:
  pcm.clear()
  // optional: start partial timer in app

onMicChunk(f32[]):
  pcm.push(chunk)   // host side
  if partial_timer_fire:
     // optional: engine.transcribe_pcm(pcm.window()) → show interim
     // use separate short timeout; ignore CANCELLED

onHoldEnd:
  result = engine.transcribe_pcm(pcm.all(), timestamps=false)
  text = cleanup_rules(result.text, Clean)
  show(text)

onCancel:
  engine.cancel()
```

### 13.2 Partials

Remain **host-driven**. Do not add `start_stream` in v1.  
Optionally document that hosts can use the same numeric defaults as `PartialWindowPolicy::dictation` (1 s min, 15 s window, 1.2 s interval) without exposing those types over FFI.

### 13.3 Process exit

```text
destroy(engine)
aurum_shutdown()
```

---

## 14. Phased delivery plan

### Phase 0 — Spec freeze (this doc)
- Review ABI table, errors, threading  
- No code required  

### Phase 1 — Façade crate skeleton
- `aurum-ffi` workspace member  
- `Engine` + `cleanup_rules` in pure Rust  
- Unit tests without C  

### Phase 2 — C API + header
- `extern "C"`, cbindgen, c smoke in CI  
- `catch_unwind`, destroy/shutdown  

### Phase 3 — Cancel + preload hardening
- Integration test with tiny model when `AURUM_INTEGRATION=1`  
- Concurrent call rejection tests  

### Phase 4 — UniFFI (optional same release)
- UDL + Kotlin/Swift generate in `examples/bindings/`  
- Document build flags  

### Phase 5 — Packaging
- Release workflow matrix for dylib/so/dll  
- iOS XCFramework only when a real consumer exists  

### Phase 6 — File helper (v1.1+)
- `transcribe_file(path)` for hosts without PCM (pulls ffmpeg dependency into app distribution story — careful)

**Explicitly deferred forever or post-1.0:** cloud provider over FFI, streaming callbacks, diarization.

---

## 15. API surface checklist (v1 freeze list)

**In:**

- [x] abi/version/shutdown  
- [x] engine create/destroy/last_error  
- [x] preload / is_model_ready  
- [x] transcribe_pcm  
- [x] cancel (+ auto reset)  
- [x] transcript accessors + free  
- [x] cleanup_rules + string free  

**Out:**

- [ ] OpenRouter  
- [ ] config.toml loading  
- [ ] ffmpeg file decode (v1.1 candidate)  
- [ ] model list (v1.1 candidate)  
- [ ] progress callbacks  
- [ ] partial clock types  
- [ ] custom greedy/beam whisper params  
- [ ] multi-handle shared context tuning  

---

## 16. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Metal teardown asserts | `shutdown` + destroy clear cache; CI on macOS |
| Tokio nested runtime deadlock | Document; dedicated runtime; never expose async |
| ABI drift C vs UniFFI | Single façade + cbindgen CI |
| Large PCM UniFFI copies | Document C API for zero-copy; UniFFI for convenience |
| Core API churn | Façade absorbs; ABI stays |
| Name confusion `aurum` crate taken | Artifact `aurum_ffi` / package `aurum-ffi`; binary libs `libaurum_ffi` |
| Hosts expect streaming | Docs + cookbook; refuse stream APIs until real design |

---

## 17. Open questions (resolve before Phase 2)

1. **Default model string** for embeds: `tiny-q5_1` vs `base-q5_1`?  
   - Recommendation: no default in C opts — **require model**; docs recommend `tiny-q5_1` for first run.
2. **Reject vs queue** concurrent transcribe?  
   - Recommendation: **reject** (`STATE`) — simpler, fewer footguns.
3. **UniFFI in v1 crate features** or separate `aurum-uniffi` crate?  
   - Recommendation: feature on `aurum-ffi` to keep one façade; heavy deps optional.
4. **Static linking whisper** into app — binary size budget?  
   - Measure before promising mobile.

---

## 18. Summary

`aurum-ffi` is the **stable product boundary** for embedders:

> **PCM in → local text out → optional rules cleanup**, with preload and cancel.

It deliberately does **not** freeze `aurum-core`. It freezes a boring, testable, fail-closed façade with a C ABI and optional UniFFI, aligned with Aurum’s tagline and the architecture already present in core (`transcribe_pcm`, `CancelFlag`, `RulesCleanup`, model cache).

**Implementation order:** façade → C → tests → UniFFI → package.  
**Quality bar:** no panics across ABI, no network by default, no logic duplication, no streaming theater.

---

## Appendix A — Mapping to existing core symbols

| FFI concept | Core symbol |
|-------------|-------------|
| Engine | `LocalWhisperProvider` + `CancelFlag` + cache path |
| preload | `LocalWhisperProvider::preload` |
| is_model_ready | `is_model_cached` / ensure without download when local_only |
| transcribe_pcm | `LocalWhisperProvider::transcribe_pcm` |
| cancel | `CancelFlag::cancel` |
| cleanup_rules | `RulesCleanup` + `CleanupStyle` |
| transcript | `TranscriptionResult` / `Segment` |
| shutdown | `providers::local::clear_context_cache` |
| sample rate | `audio::WHISPER_SAMPLE_RATE` |

## Appendix B — Minimal Swift-shaped usage (informative)

```swift
// Pseudocode — bindings not shipped yet
let engine = try AurumEngine(cacheDir: cacheURL.path, localOnly: true)
try engine.preload(model: "tiny-q5_1")
let transcript = try engine.transcribePcm(samples: floats, model: "tiny-q5_1", language: "en")
let cleaned = try Aurum.cleanupRules(text: transcript.text, style: .clean)
engine.cancel() // if needed mid-flight from another queue
```

## Appendix C — Related docs

- [Architecture](architecture.md)  
- [Library integration](../library/integration.md)  
- [Partials](../library/partials.md)  
- [Cleanup](../guide/cleanup.md)  
