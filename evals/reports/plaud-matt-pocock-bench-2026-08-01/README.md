# Plaud × YouTube × Aurum STT bench (2026-08-01)

Snapshot of offline evaluation work for the Matt Pocock lecture recorded on Plaud
and sourced from YouTube.

- **Video:** https://www.youtube.com/watch?v=n0VhIVtviC0
- **Plaud file id:** `42e86284875bf900d98cb7e83ce3b7cb`
- **Aurum:** 0.0.17

## Start here

| Doc | Contents |
|-----|----------|
| [EVAL_REPORT.md](EVAL_REPORT.md) | Dual-ref leaderboard (Plaud + YouTube) + chunked OpenRouter + gold kit |
| [CHUNKED_OPENROUTER_REPORT.md](CHUNKED_OPENROUTER_REPORT.md) | Aurum CLI OpenRouter path with 210s chunking |
| [gold_subset/README.md](gold_subset/README.md) | Human gold annotation kit (fill `gold.txt`) |

## Reproduce (high level)

```bash
# Dual-ref rescore of existing hypotheses/
python3 run_dual_eval.py

# Rebuild gold clips (needs plaud_audio.mp3 + ffmpeg)
python3 build_gold_subset.py

# Chunked OpenRouter via Aurum CLI (needs OPENROUTER_API_KEY + aurum bin)
export AURUM_BIN=/path/to/aurum
python3 run_chunked_openrouter.py
```

## Notes

- WER is agreement with Plaud/YouTube auto refs, **not** human-verified gold until `gold_subset` is filled.
- `chunks/` and `chunked_work/` were omitted (regenerable); `plaud_audio.mp3` is included.
- Do not commit API keys; scripts read `OPENROUTER_API_KEY` from the environment only.
