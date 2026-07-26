# Contest result

## Agent
pi

## Summary
- Implemented local-default TTS MVP: `aurum tts` text → mono WAV on the existing `aurum-stt` binary
- Added `aurum-core` module `tts/` behind cargo feature `tts` (default on): provider trait, KittenTTS ONNX local engine, catalogue + SHA-256 pins + download lock, WAV writer, validation/limits/timeout/peak guard
- Config `[tts]` + env overrides `AURUM_TTS_MODEL` / `AURUM_TTS_VOICE` / `AURUM_TTS_LANGUAGE`
- CLI: synthesis flags, atomic write, overwrite policy, `--emit-json` honesty schema, nested `aurum tts models` / `aurum tts voices`
- Tests: unit coverage (validate/wav/catalogue) + integration synth path (`tts_synth`) producing real WAV
- Docs: `docs/guide/tts.md` (license matrix, pins, agent template), CLI/arch/config/SECURITY/CHANGELOG + mkdocs nav
- STT paths unchanged (`aurum models`, cleanup, etc.)

## Engine / voice choice + license
- **Engine:** ONNX Runtime via `ort` 2.0.0-rc.11 (MIT/Apache) — CPU, download-binaries
- **Model pack:** KittenTTS **nano int8** (`kitten-nano-int8`) from Hugging Face `KittenML/kitten-tts-nano-0.8-int8` (**Apache-2.0**), ~25 MB ONNX + voices.npz
- **Default voice:** `Luna` (`expr-voice-3-f`); also Bella/Jasper/Bruno/Rosie/Hugo/Kiki/Leo
- **G2P:** `misaki-rs` **0.3.0 with default features off** (**MIT**) — no espeak-ng / GPL on the link path
- **Why not Piper default:** Piper-class phonemization typically needs eSpeak NG (GPL), which would break the MIT binary gate. KittenTTS + MIT G2P is the honest MIT-safe real neural path.
- **Output:** in-process mono PCM 16-bit WAV @ 24 kHz via `hound` (no ffmpeg)

Pinned SHA-256:
- `kitten_tts_nano_v0_8.onnx` = `f7b0afcbee92870b32b8e0276d855b954dc25470c9f051b376ac7eee537c76fc`
- `voices.npz` = `8aa7cee235abb0739cb51e6559685f65a4dacd95568833d05699b1633f519b3f`
- `config.json` = `b66006ccbeccd4de5fc3c9272059c47f5725df7215fd889785c03602652fab64`

## Commands run + outcomes
```text
cargo fmt --all -- --check                          # pass
cargo clippy --workspace --all-targets --locked -- -D warnings  # pass
cargo test --workspace --locked                     # pass (incl. tts_synth 2 tests)
cargo run -p aurum-stt -- tts "Hello from aurum" \
  --output-file /tmp/aurum-tts-smoke.wav --emit-json
  # → playable mono 24 kHz WAV + honesty JSON (duration_ms ~2191)
file /tmp/aurum-tts-smoke.wav
  # RIFF WAVE audio, Microsoft PCM, 16 bit, mono 24000 Hz
cargo run -p aurum-stt -- tts models                # lists kitten-nano-int8 cached
cargo run -p aurum-stt -- tts voices                # lists 8 voices
cargo run -p aurum-stt -- models                    # STT catalogue still works
cargo run -p aurum-stt -- tts "" -O /tmp/x.wav      # exit 2 empty text
cargo run -p aurum-stt -- tts hi -O /tmp/aurum-tts-smoke.wav  # exit 2 no --force
.venv/bin/mkdocs build --strict                     # pass
```

Offline note: with pack cached under `~/.cache/aurum/tts/kitten-nano-int8/`, synthesis runs with **no network**. Cold cache downloads once with fail-closed SHA-256.

## Acceptance checklist
- [x] `aurum tts "Hello from aurum" --output-file <path>.wav` writes playable mono WAV with cache warm and no network on that run
- [x] Cold cache: download is explicit, SHA-256 pinned, fail-closed offline when missing (`--local-only` / missing pack)
- [x] `aurum tts models` and `aurum tts voices` list catalogue + cache status
- [x] Exactly one text source; empty text → exit 2
- [x] max chars + timeout enforced (config defaults 5000 / 120000 ms; truncate sets `text_truncated`)
- [x] Atomic write; no silent overwrite without `--force`
- [x] `--emit-json` matches schema; `output_path` absolute when possible
- [x] License matrix documented; MIT binary story intact (no GPL default engine link)
- [x] CI/unit/integration coverage; workspace tests green
- [x] Docs (guide + CLI/arch + security + command-provider example) + CHANGELOG
- [x] STT paths unchanged and still smoke-clean

## Known gaps
- No multi-language G2P beyond English misaki lexicon (OOV without espeak may spell/skip unknowns — documented)
- TTS not exposed via `aurum-ffi` (explicitly out of scope)
- No speaking-rate sample-rate resampling beyond model `speed` input
- `timeout` is checked after blocking inference completes (cooperative cancel flag is honored at start of synth; mid-infer cancel depends on ORT)

## Diff stats
```text
 CHANGELOG.md                           |   7 +-
 Cargo.lock                             | 475 +++++++++++++++++++++++-
 Cargo.toml                             |   5 +
 SECURITY.md                            |  19 +-
 crates/aurum-core/Cargo.toml           |  13 +-
 crates/aurum-core/src/config.rs        | 115 ++++++
 crates/aurum-core/src/lib.rs           |  10 +
 crates/aurum-core/src/tts/catalogue.rs | 639 +++++++++++++++++++++++++++++++++
 crates/aurum-core/src/tts/local.rs     | 374 +++++++++++++++++++
 crates/aurum-core/src/tts/mod.rs       |  39 ++
 crates/aurum-core/src/tts/npz.rs       | 195 ++++++++++
 crates/aurum-core/src/tts/provider.rs  |  78 ++++
 crates/aurum-core/src/tts/tokenize.rs  |  70 ++++
 crates/aurum-core/src/tts/validate.rs  | 158 ++++++++
 crates/aurum-core/src/tts/wav.rs       | 135 +++++++
 crates/aurum-core/tests/tts_synth.rs   |  72 ++++
 crates/aurum/src/cli.rs                | 281 ++++++++++++++-
 docs/development/architecture.md       |  33 +-
 docs/getting-started/cli.md            |  28 ++
 docs/guide/configuration.md            |  21 +-
 docs/guide/tts.md                      | 154 ++++++++
 mkdocs.yml                             |   1 +
 22 files changed, 2890 insertions(+), 32 deletions(-)
```
(Plus `CONTEST_RESULT.md` in the contest commit.)
