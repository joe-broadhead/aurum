# Gold subset — human annotation template

- **generated:** 2026-08-01T18:55:20Z
- **source video:** https://www.youtube.com/watch?v=n0VhIVtviC0
- **clips:** 9 (8 hard + 1 easy controls)

## How to annotate

1. Open `clips/clip_XX/audio.wav` (or play while reading).
2. Read `plaud_draft.txt` and `youtube_draft.txt` as **hints only**.
3. Write what you hear into `gold.txt` (one paragraph, no timestamps).
4. Optional: note uncertainty in `notes.txt`.
5. When all gold.txt filled, run `python3 score_against_gold.py`.

## Clip index

| ID | Kind | Time | Hardness | Cross-WER | Multi-hyp |
