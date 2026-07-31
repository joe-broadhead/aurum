# Batch workflow

```bash
aurum batch INPUT --output-dir OUT [options]
```

| Flag | Meaning |
|------|---------|
| `--recursive` | Walk subdirectories |
| `--resume` | Continue existing manifest |
| `--retry-failed` | Re-run failed items |
| `--dry-run` | Manifest only |
| `--profile` / `--model` | Model selection |
| `-o txt\|srt\|json` | Per-item format |
| `--json` | Machine-readable summary |

Manifest schema version is `1` (`aurum-batch-manifest.json`).
