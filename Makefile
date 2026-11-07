SHELL := /bin/bash

.PHONY: build install test lint fmt docs docs-serve clean ci release-dry-run version-check eval-dictation eval-dictation-check

build:
	cargo build -p aurum-stt --release --locked

install:
	cargo install --path crates/aurum --locked --force

test:
	cargo test --workspace --locked

lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings

fmt:
	cargo fmt --all -- --check

# Offline corpus validation: manifest, references, durations, sha256, licences.
eval-dictation-check:
	python3 scripts/run_dictation_eval.py --check

# Real WER gate. Needs a built binary and a cached model; honours AURUM_CACHE
# and AURUM_EVAL_MODEL (default tiny-q5_1).
eval-dictation: build
	python3 scripts/run_dictation_eval.py --aurum target/release/aurum --model "$${AURUM_EVAL_MODEL:-tiny-q5_1}"

docs:
	python3 -m pip install -q -r docs/requirements.txt
	mkdocs build --strict

docs-serve:
	python3 -m pip install -q -r docs/requirements.txt
	mkdocs serve

clean:
	cargo clean
	rm -rf site .venv

version-check:
	@./scripts/version_check.sh

ci: fmt lint test version-check eval-dictation-check
	@echo "ci ok"

release-dry-run: version-check build
	@echo "Release dry-run OK (no tag, no publish)"

# Full fail-closed pre-tag gate (JOE-1640). May install Python deps for mkdocs.
release-gate:
	@./scripts/release_gate.sh
