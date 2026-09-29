.PHONY: check fmt clippy test wasm build bench resource-gate wasm-size setup mutants

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

## Resource-usage regression gate (#119): meter every hot entry point of the
## release WASM and compare against resource_baseline.toml. CI runs this with
## a pinned toolchain (.github/workflows/resource-gate.yml); numbers depend on
## the exact WASM, so a different local rustc may show small drift.
## To accept an intended change: scripts/check_resource_budget.sh --update
resource-gate: wasm
	scripts/check_resource_budget.sh

## Release WASM size gate (#122): fail if `make wasm`'s output grew more than
## max_growth_pct over wasm_size.toml's baseline, or exceeds its ceiling (75%
## of Soroban's contract_max_size_bytes). Accept intended growth with:
##   scripts/check_wasm_size.sh --update
wasm-size: wasm
	scripts/check_wasm_size.sh

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
