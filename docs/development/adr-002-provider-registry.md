# ADR-002: Direction-specific providers with a shared registry (JOE-1933)

**Status:** Accepted  
**Date:** 2026-08-02  
**Epic:** [JOE-1932](https://linear.app/joe-broadhead/issue/JOE-1932) / GitHub #59  
**Related:** JOE-1934–JOE-1943 (transport, config, capabilities, verticals)

## Context

Aurum already has object-safe `TranscriptionProvider` (STT) and `SynthesisProvider`
(TTS) traits. Adding remote TTS and more STT backends by extending CLI `match`
arms, engine helpers, capability tables, and OpenRouter-shaped config would create
an O(N providers × surfaces) mess and let library/CLI behavior drift.

A single universal “speech provider” trait would be equally wrong: STT and TTS do
not share inputs, outputs, timestamp truth, voice semantics, or lifecycle.

## Decision

1. **Keep direction traits separate.** STT stays in `providers/`; TTS stays in
   `tts/`. Shared concerns (identity, construction, secrets scoping, transport,
   capabilities, metrics, conformance) live in a **provider platform** layer.
2. **Validated `ProviderId`** is the only core-boundary identity (not free strings).
3. **Direction-specific factories** (`TranscriptionProviderFactory` /
   `SynthesisProviderFactory`) register under an immutable-after-build
   **`ProviderRegistry`**.
4. **`ProviderBuildContext`** supplies cache paths, governor/metrics/pools, and
   **provider-scoped** secrets only. A factory never receives every vendor key.
5. **Only compiled/reviewed factories** are registered by default. No dynamic
   plugin loading and no config-driven arbitrary HTTP provider.
6. **Preserve 0.x convenience APIs** (`local_whisper()`, `local_tts()`,
   `LocalWhisperProvider::new`, `OpenRouterProvider::new`). Engine high-level
   routing migrates in JOE-1938; this ADR does not require that cut-over yet.

## Consequences

### Positive

- New named providers (OpenAI, ElevenLabs, xAI) register once and pass
  conformance without editing CLI/engine match arms for each surface.
- Secret isolation is expressible in the type of the build context.
- Capabilities can migrate onto factory/model descriptors (JOE-1936) without
  flattening STT/TTS semantics.

### Negative / deferred

- Two factory traits instead of one (intentional).
- Full engine/CLI routing still lives on temporary branches until JOE-1938.
- Provider-shaped config (`[providers.<id>]`) is JOE-1935; build context
  currently accepts explicit scoped fields until that lands.

### Invariants

- `local` remains the product default; remote requires deliberate selection.
- `local_only` must reject remote factories before payload preparation.
- Remote TTS audio is normalized to mono PCM via
  [`guide/remote-audio.md`](../guide/remote-audio.md) (JOE-1937) before it
  enters `SynthesisResult`.
- No silent local→cloud fallback.

## Alternatives considered

| Alternative | Why rejected |
|-------------|--------------|
| One `SpeechProvider` trait | Meaningless optional methods or wrong shared semantics |
| Keep CLI/engine match arms forever | Does not scale; library embedders diverge |
| Dynamic `.so` plugins | Trust boundary; not in first release of JOE-1932 |
| Generic credentialed URL provider | Violates named-origin security posture |

## Implementation map

| Piece | Location |
|-------|----------|
| Identity + registry + factories | `aurum_core::provider_platform` |
| Built-in registration | `provider_platform::builtin` |
| ADR (this document) | `docs/development/adr-002-provider-registry.md` |
| Transport policies | JOE-1934 |
| Config migration | JOE-1935 |
| Capabilities ownership | JOE-1936 |
| Engine/CLI routing | JOE-1938 |
