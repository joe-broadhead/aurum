# Do not

- Invent CLI flags or config keys.
- Change the product default model without an evidence review.
- Select experimental models (`large-v3-q5_0`) via profiles.
- Enable network or remote providers implicitly.
- Paste secrets, audio files, or full transcripts into chat by default.
- Claim WER/RTF numbers without a versioned report under `evals/` / bench output.
- Expand FFI ABI casually — treat it as a stability boundary.
- Improvise full STT/TTS provider advice here — use **`skills/aurum-speech/`**.
