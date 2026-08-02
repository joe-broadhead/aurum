# Provider evidence pack (JOE-2223)

Machine-readable evidence for support-tier claims. **`supported` requires a
fresh (≤30 days) passing protected inference smoke** for remote routes. Local
routes are always evidence-backed offline.

## Layout

| Path | Role |
|------|------|
| `index.json` | Supported claims required for release + experimental route list |
| `local-*.json` | Offline local STT/TTS evidence |
| `*.json` | Protected smoke records (no payloads/keys) |

## Gate

```bash
./scripts/check_provider_evidence.sh
```

Fails if any `required_for_release` claim lacks fresh passing evidence.

## Adding remote supported evidence

1. Run protected smoke (synthetic fixtures only).
2. Write a redacted `ProviderEvidenceRecord` JSON here.
3. Add/update the claim in `index.json` with `support_tier: supported`.
4. Update product surfaces + changelog if promoting/demoting.

Discovery catalogue output **never** auto-expands the trusted registry.
