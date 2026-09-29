.PHONY: check fmt clippy test wasm build bench setup mutants profile-diff

# Run the full local check suite — mirrors the CI job exactly.
# A passing `make check` guarantees the same commit will pass CI.
check: fmt clippy test wasm

# ── Individual targets ────────────────────────────────────────────────────────

## Verify formatting matches rustfmt.toml (read-only, same as CI).
fmt:
	cargo fmt --all -- --check

## Lint with clippy; deny all warnings.
clippy:
	cargo clippy --all-targets -- -D warnings

## Run the test suite (native host, not wasm).
test:
	cargo test --verbose

## Build the release wasm artefact (confirms the cdylib target compiles).
wasm:
	cargo build --target wasm32-unknown-unknown --release

## Plain debug build (quick sanity check).
build:
	cargo build --verbose

## Run event-emission benchmarks and print the [bench] summary lines.
## The regression ceilings in src/bench_events.rs are asserted as part of
## `make test`; this target surfaces the raw numbers for local inspection.
## Grep-friendly: all cost lines are prefixed with "[bench]".
bench:
	cargo test bench_ -- --nocapture 2>&1 | grep -E '^\[bench\]|^test bench_'

## Differential test: run the behaviour trace under `release` and
## `release-with-logs` and fail on any divergence (issue #131). Slow: builds
## the test binary twice with LTO. `--self-test` proves detection works.
profile-diff:
	scripts/diff_profiles.sh --self-test
	scripts/diff_profiles.sh

## One-time contributor setup: install the pre-commit hook.
setup:
	git config core.hooksPath .git-hooks
	@echo "Pre-commit hook installed. Run 'make check' to verify your environment."

## Run cargo-mutants against core contract logic (issue #128).
##
## Requires cargo-mutants to be installed:
##   cargo install cargo-mutants
##
## Exit code 0  — all mutants killed (100% kill rate).
## Exit code 2  — some mutants survived (see mutants.out/).
## Exit code 3  — some tests timed out.
##
## CI uses a minimum-kill-rate budget (see .github/workflows/rust.yml).
mutants:
	cargo mutants --in-place
