# aurum-ffi

C ABI façade over [`aurum-core`](https://crates.io/crates/aurum-core) for native
**STT + TTS + cleanup** embedders (ABI v2, pre-1.0 / provisional).

**Speech both ways. On-device by default.**

| Capability | API |
|------------|-----|
| Engine | `aurum_engine_create` / `aurum_engine_close` / `aurum_engine_destroy` |
| Models | `preload`, `is_model_ready` |
| Decode | `aurum_engine_transcribe_pcm` (16 kHz mono f32) |
| Jobs | `aurum_job_start_{preload,transcribe,cleanup,tts}`, poll/wait/cancel/take/free |
| TTS | Job API + `aurum_audio_*` handles (feature `tts`, on by default) |
| Cancel | `aurum_engine_cancel`, `aurum_job_cancel` |
| Cleanup | `aurum_cleanup_rules` (on-device, no engine required) |
| Doctor | `aurum_doctor_json`, `aurum_capabilities` |
| Teardown | `aurum_engine_shutdown` (engine-local); `aurum_shutdown` / `aurum_shutdown_ex` (process) |

Header: [`include/aurum.h`](include/aurum.h)

### Lifecycle notes (JOE-1647)

- Prefer **`aurum_engine_close`**: waits for exclusive blocking ops, export-boundary
  calls (including `last_error` writes), and jobs; frees only on success.
- **`aurum_engine_destroy`**: same drain; **frees only after successful drain**.
  On BUSY the pointer remains valid — retry close; do not assume free.
- Hosts must serialize destroy/close with other calls on the same handle.
- Every fallible out-pointer is nulled **before** admission / validation.

### Downstream C/C++ examples

`examples/job_cleanup.c` (C11) and `examples/engine_raii.cpp` (C++17) are built
and run in CI on **Linux and macOS** against a release staticlib with
`--no-default-features` (STT/cleanup only, no ORT). **Windows MSVC host-link** of
those examples is deferred; core tests still run on Windows.

```bash
cargo build -p aurum-ffi --release
# STT-only staticlib (matches CI native examples):
cargo build -p aurum-ffi --release --no-default-features
```

### Official SDK archives (JOE-2225)

Prefer a **versioned release SDK bundle** over linking a workspace `target/`:

```bash
./scripts/package_native_sdk.sh --features none
./scripts/qualify_native_sdk_bundle.sh --archive dist/native-sdk/aurum-sdk-*.tar.gz
```

Layout: `include/`, `lib/`, `cmake/`, `pkg-config/`, `examples/`, `SDK_MANIFEST.json`.
Remote providers remain unsupported through the C ABI. See
[Native SDK](https://joe-broadhead.github.io/aurum/library/native-sdk/).

```toml
aurum-ffi = "0.0.22"
```

See [Native embeds](https://joe-broadhead.github.io/aurum/library/ffi/).

## License

MIT
