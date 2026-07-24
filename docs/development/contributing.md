# Contributing

See the root [CONTRIBUTING.md](https://github.com/joe-broadhead/aurum/blob/main/CONTRIBUTING.md).

Quick loop:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
./Scripts/version_check.sh
mkdocs build --strict
```
