# Contributing

See the root [CONTRIBUTING.md](https://github.com/joe-broadhead/aurum/blob/master/CONTRIBUTING.md).

Quick loop:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
./scripts/version_check.sh
.venv/bin/mkdocs build --strict
```
