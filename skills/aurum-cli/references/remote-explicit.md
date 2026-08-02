# Explicit remote (pointer)

Remote STT/TTS is fully documented in **`skills/aurum-speech/`**:

- STT matrix: `../aurum-speech/references/stt.md`
- TTS matrix: `../aurum-speech/references/tts.md`
- Hard rules: `../aurum-speech/references/do-not.md`

Only when the user asks for cloud speech, and only with an explicit
`--provider` plus the matching env key. Never select remote because a key exists.
