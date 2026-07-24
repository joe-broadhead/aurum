# aurum

CLI for [Aurum](https://github.com/joe-broadhead/aurum).

**Audio in. Text out. On-device by default.**

```bash
cargo install --path crates/aurum --locked
# or install a binary from GitHub Releases

aurum models
aurum meeting.m4a --model tiny-q5_1
aurum meeting.m4a --cleanup clean
echo "um hello" | aurum cleanup -s clean
```

Library consumers should depend on **[`aurum-core`](../aurum-core)**, not this crate.

Docs: <https://joe-broadhead.github.io/aurum/>

## License

MIT
