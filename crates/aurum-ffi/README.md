# aurum-ffi

C ABI façade over [`aurum-core`](https://crates.io/crates/aurum-core) for native **STT** embedders.

**Speech both ways. On-device by default.**  
(This crate exposes the **listen** path: PCM → text. TTS remains CLI/library Rust for now.)

| Capability | API |
|------------|-----|
| Engine | `aurum_engine_create` / `destroy` |
| Models | `preload`, `is_model_ready` |
| Decode | `aurum_engine_transcribe_pcm` (16 kHz mono f32) |
| Cancel | `aurum_engine_cancel` |
| Cleanup | `aurum_cleanup_rules` (on-device) |
| Teardown | `aurum_shutdown` (Metal-safe) |

Header: [`include/aurum.h`](include/aurum.h)

```bash
cargo build -p aurum-ffi --release
```

```toml
aurum-ffi = "0.0.2"
```

See [Native embeds](https://joe-broadhead.github.io/aurum/library/ffi/).

## License

MIT
