# aurum-stt

CLI for [Aurum](https://github.com/joe-broadhead/aurum).

**Speech both ways. On-device by default.**

```bash
cargo install aurum-stt
# or: cargo install --path crates/aurum --locked

aurum models
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --cleanup clean
echo "um hello" | aurum cleanup -s clean

aurum tts "Hello from aurum" -O /tmp/hello.wav
aurum tts voices
```

Library: **[`aurum-core`](../aurum-core)**. Native STT embeds: **[`aurum-ffi`](../aurum-ffi)**.

Docs: <https://joe-broadhead.github.io/aurum/>

## License

MIT
