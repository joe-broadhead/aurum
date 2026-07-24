# aurum

Command-line interface for [Aurum](https://github.com/joe-broadhead/aurum) — on-device speech-to-text.

```bash
cargo install --path crates/aurum --locked
# or download a release binary from GitHub Releases

aurum models
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --cleanup clean
echo "um hello" | aurum cleanup -s clean
```

Library consumers should depend on **`aurum-core`**, not this crate.

Docs: <https://joe-broadhead.github.io/aurum/>

## License

MIT
