.PHONY: check fmt clippy test wasm build setup

# Run the full local check suite — mirrors the CI job exactly.
# A passing `make check` guarantees the same commit will pass CI.
check: fmt clippy test wasm

# ── Individual targets ────────────────────────────────────────────────────────

## Verify formatting matches rustfmt.toml (read-only, same as CI).
fmt:
	cargo fmt --all -- --check

## Lint with clippy; deny all warnings except todo! placeholders.
clippy:
	cargo clippy --all-targets -- -D warnings -A clippy::todo

## Run the test suite (native host, not wasm).
test:
	cargo test --verbose

## Build the release wasm artefact (confirms the cdylib target compiles).
wasm:
	cargo build --target wasm32-unknown-unknown --release

## Plain debug build (quick sanity check).
build:
	cargo build --verbose

## One-time contributor setup: install the pre-commit hook.
setup:
	git config core.hooksPath .git-hooks
	@echo "Pre-commit hook installed. Run 'make check' to verify your environment."
