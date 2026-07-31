# Threat model and deployment profiles (JOE-1637)

## Assets

| Asset | Sensitivity |
|-------|-------------|
| User audio / transcripts / TTS text | High (PII / confidential content) |
| API keys (`OPENROUTER_API_KEY`) | High |
| Model/voice caches under user cache dir | Medium (integrity) |
| Output files (transcripts, WAV) | Medium–High |
| Release binaries / crates | High (supply chain) |

## Actors

- Local user (trusted account on single-user CLI profile)
- Host application / agent (may be multi-tenant if the host is)
- Remote OpenRouter (only when explicitly selected)
- Network attacker (MITM, malicious model hosts)
- Malicious local filesystem (symlink races, hostile media)

## Trust boundaries

1. **Process boundary** — Aurum cannot trust other processes on a shared host.
2. **Network boundary** — default local path does not open network sockets.
3. **Filesystem boundary** — output transaction rejects symlink clobber; model packs verify digests.
4. **Native code boundary** — whisper.cpp / ONNX Runtime are code-adjacent trust.
5. **FFI boundary** — host must honor ownership/free rules; invalid foreign pointers are not fully defensively safe.

## Deployment profiles

| Profile | Assumptions | Required controls |
|---------|-------------|-------------------|
| Single-user local CLI | Trusted OS account | Default local provider; cache integrity; no secrets in logs |
| App embed (desktop/mobile) | Untrusted media/text | Input size caps; cancel; no payload logging |
| Native plugin / multi-caller | Concurrent hosts | Engine isolation; job API; ResourceGovernor |
| Server / multi-tenant | Hostile tenants | Outer sandbox; no shared cache identity; remote only with policy |

## Abuse cases (selected)

- Path traversal via `--output-file` / pack dirs → rejected by validators + symlink policy
- Digest mismatch / poisoned cache → fail closed; quarantine tooling for STT
- Nested Tokio from FFI host → use jobs (ABI v2)
- Mutable remote model refs → custom catalogue requires pins; no auto-trust HF cards
- Workflow supply-chain compromise → pin Actions SHAs; fail-closed tag checkout

## Non-goals

- Multi-tenant isolation inside a single process without host cooperation
- Making arbitrary ONNX execution “safe”
- Replacing an outer OS sandbox for hostile multi-tenant media
