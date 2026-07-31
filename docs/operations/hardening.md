# Hardening baseline (JOE-1637)

## Operator checklist

1. Prefer **local** STT/TTS; set `local_only` for offline embeds.
2. Prefer env for secrets (`OPENROUTER_API_KEY`); never log `Debug` secrets (redacted).
3. Run `aurum doctor` after install; fix `!!` failures before production use.
4. Keep ffmpeg on PATH for file STT; pin system packages where possible.
5. Restrict cache directory permissions (`0700` recommended for multi-user hosts).
6. For custom TTS packs: `trust=verified` with digests only; avoid `local_unverified`.
7. Verify release downloads with `SHA256SUMS` + `scripts/verify_release_assets.sh`.
8. Review `PROVENANCE.txt` / SBOM inventory on each release.

## Embedder checklist

1. Prefer **job APIs** from event-loop threads (ABI v2).
2. Destroy engines after `aurum_engine_shutdown`; free results once.
3. Call `aurum_shutdown_ex` only after engines are destroyed.
4. Do not pass untrusted paths to output without host-side sandboxing.
5. Install your own tracing subscriber; library does not install a global one.

## CI / release checklist

1. `cargo audit` and `cargo deny check` green on the tag.
2. Actions pinned to full commit SHAs (see workflow comments).
3. Tag checkout fails closed (exact tag object).
4. SBOM inventory + checksums attached to GitHub Release.
5. `./scripts/release_gate.sh` on a clean tree before prepare/tag.

## Disclosure

See root [SECURITY.md](https://github.com/joe-broadhead/aurum/blob/master/SECURITY.md) for reporting and the disclosure rehearsal procedure.
