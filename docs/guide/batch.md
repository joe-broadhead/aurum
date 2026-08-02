# Batch transcription (JOE-2220)

`aurum batch` processes a file or folder with a **versioned, content-addressed**
resume manifest.

## Contract (manifest v2)

* Full **SHA-256 of complete source bytes** (not the first MiB)
* Full **SHA-256 of committed output** after success
* Canonical **operation fingerprint** (provider, model, language, timestamps,
  format, cleanup, profile, local-only, …)
* Manifest publishes via **`OutputTransaction`** (replace mode, symlink-safe)
* **Single-writer lock** (`aurum-batch.lock`) — stale locks are never broken
  automatically
* **Resume decision table** with explicit `stale_source`, `stale_configuration`,
  `stale_output`, and `interrupted` states
* Provider validation via **`ProviderRegistry`** (not a hard-coded subset)
* v1 manifests are **rejected** (never silently trusted)

## Commands

```bash
# Fresh batch
aurum batch ./clips --output-dir ./out --model tiny-q5_1

# Exact-match resume only
aurum batch ./clips -O ./out --resume

# Retry failures / interrupted
aurum batch ./clips -O ./out --resume --retry-failed

# Opt in when source/config/output changed
aurum batch ./clips -O ./out --resume --reprocess-changed

# Report decisions without transcription
aurum batch ./clips -O ./out --resume --verify-only --json
```

## Success state machine

1. Compute full source identity  
2. Mark `running` and persist  
3. Transcribe and transactionally publish transcript  
4. Hash committed output  
5. Mark `succeeded` with output identity and persist  

A crash before step 5 never claims success.

## Migration from v1

v1 manifests used a partial fingerprint named like SHA-256. Aurum **refuses** to
load them as v2. Start a new `--output-dir` or rebuild the batch after computing
full digests. Never infer full content identity from the old partial value.

## Security

* Symlinks rejected for manifests and outputs  
* Error messages bounded; no API keys, transcripts, or private notes in the
  manifest  
* Lock metadata: PID, run id, start time only  
