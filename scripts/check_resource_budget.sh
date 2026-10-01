#!/usr/bin/env bash
# Resource-usage regression gate (#119).
#
# Meters every hot entry point of the release WASM under Soroban's budget and
# fails if any scenario's cpu_insns / mem_bytes exceeds resource_baseline.toml
# by more than the threshold. See src/bench_resources.rs and COST_MODEL.md §12.
#
# Usage:
#   scripts/check_resource_budget.sh [--baseline PATH] [--wasm PATH]
#                                    [--threshold PCT] [--update]
#                                    [--inject SCENARIO:PCT] [--expect-fail]
#
#   --update       rewrite the baseline from current measurements (intentional
#                  regressions/improvements; commit the result with a reason)
#   --inject       inflate one scenario's cpu_insns by PCT% (gate self-test)
#   --expect-fail  succeed only if the gate fails (CI negative fixture)
#
# Expects the release WASM to already exist (CI builds once, then reuses).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
BASELINE="${ROOT}/resource_baseline.toml"
WASM="${TARGET_DIR}/wasm32-unknown-unknown/release/synapse_core_contract.wasm"
EXPECT_FAIL=0
UPDATE=0
INJECT=""
THRESHOLD=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --baseline) BASELINE="$2"; shift 2 ;;
    --wasm) WASM="$2"; shift 2 ;;
    --threshold) THRESHOLD="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    --inject) INJECT="$2"; shift 2 ;;
    --expect-fail) EXPECT_FAIL=1; shift ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -f "$WASM" ]]; then
  echo "ERROR: release WASM not found at:" >&2
  echo "  $WASM" >&2
  echo "Build it once with: cargo build --target wasm32-unknown-unknown --release" >&2
  exit 1
fi

export SYNAPSE_RESOURCE_GATE=1
export SYNAPSE_RESOURCE_WASM="$WASM"
export SYNAPSE_RESOURCE_BASELINE="$BASELINE"
if [[ -n "$THRESHOLD" ]]; then export SYNAPSE_RESOURCE_THRESHOLD_PCT="$THRESHOLD"; fi
if [[ -n "$INJECT" ]]; then export SYNAPSE_RESOURCE_INJECT="$INJECT"; fi
if [[ "$UPDATE" -eq 1 ]]; then export SYNAPSE_RESOURCE_UPDATE=1; fi

echo "rustc: $(rustc --version)"
echo "wasm:  $WASM ($(wc -c < "$WASM") bytes, sha256 $(sha256sum "$WASM" | cut -c1-16))"

set +e
cargo test --lib -- --nocapture --exact bench_resources::resource_gate_release_wasm
STATUS=$?
set -e

if [[ "$EXPECT_FAIL" -eq 1 ]]; then
  if [[ "$STATUS" -ne 0 ]]; then
    echo "OK: resource gate failed as expected (regression fixture)."
    exit 0
  fi
  echo "ERROR: expected the resource gate to fail, but it passed." >&2
  exit 1
fi

if [[ "$STATUS" -ne 0 ]]; then
  echo "" >&2
  echo "ERROR: resource-usage gate failed — see the REGRESSION lines above." >&2
  exit 1
fi

if [[ "$UPDATE" -eq 1 ]]; then
  echo "Baseline updated: $BASELINE — review the diff and commit it."
else
  echo "OK: all entry points within budget of $BASELINE"
fi
