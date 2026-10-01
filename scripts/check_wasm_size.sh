#!/usr/bin/env bash
# Release WASM binary-size gate (#122).
#
# Compares the size of `make wasm`'s output against wasm_size.toml:
#   * regression: fail if it grew more than max_growth_pct over baseline_bytes
#   * ceiling:    fail if it exceeds ceiling_bytes (headroom below Soroban's
#                 contract_max_size_bytes, so in-place upgrades stay possible)
#
# Usage:
#   scripts/check_wasm_size.sh [--wasm PATH] [--config PATH]
#                              [--ceiling BYTES] [--update] [--expect-fail]
#
#   --update       rewrite baseline_bytes to the current size (intended growth;
#                  commit wasm_size.toml with a justification)
#   --ceiling      override ceiling_bytes (gate self-test)
#   --expect-fail  succeed only if the gate fails (CI negative fixture)
#
# Expects the release WASM to already exist (CI builds once, then reuses).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
CONFIG="${ROOT}/wasm_size.toml"
WASM="${TARGET_DIR}/wasm32-unknown-unknown/release/synapse_core_contract.wasm"
CEILING_OVERRIDE=""
UPDATE=0
EXPECT_FAIL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --wasm) WASM="$2"; shift 2 ;;
    --config) CONFIG="$2"; shift 2 ;;
    --ceiling) CEILING_OVERRIDE="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    --expect-fail) EXPECT_FAIL=1; shift ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -f "$WASM" ]]; then
  echo "ERROR: release WASM not found at:" >&2
  echo "  $WASM" >&2
  echo "Build it once with: make wasm" >&2
  exit 1
fi

# Read an integer `key = value` from the config (underscores allowed).
cfg() {
  local v
  v="$(sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*\([0-9_]*\).*/\1/p" "$CONFIG" | tr -d _)"
  if [[ -z "$v" ]]; then
    echo "ERROR: $CONFIG has no integer '$1'" >&2
    exit 1
  fi
  echo "$v"
}

BASELINE="$(cfg baseline_bytes)"
GROWTH_PCT="$(cfg max_growth_pct)"
CEILING="${CEILING_OVERRIDE:-$(cfg ceiling_bytes)}"
LIMIT="$(cfg network_limit_bytes)"
SIZE="$(wc -c < "$WASM" | tr -d ' ')"

# Signed percentage with one decimal, from integer arithmetic.
pct() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%+.1f%%", (a - b) * 100 / b }'; }

echo "wasm:     $WASM"
echo "size:     $SIZE bytes ($(pct "$SIZE" "$BASELINE") vs baseline $BASELINE)"
echo "ceiling:  $CEILING bytes ($((SIZE * 100 / CEILING))% used)"
echo "network:  $LIMIT bytes contract_max_size_bytes ($((SIZE * 100 / LIMIT))% used)"

if [[ "$UPDATE" -eq 1 ]]; then
  sed -i.bak "s/^baseline_bytes[[:space:]]*=.*/baseline_bytes = $SIZE/" "$CONFIG"
  rm -f "$CONFIG.bak"
  echo "Baseline updated: baseline_bytes = $SIZE in $CONFIG — commit it with a reason."
  exit 0
fi

FAIL=0
# Growth allowed: baseline * (1 + pct/100), compared in integers.
if (( SIZE * 100 > BASELINE * (100 + GROWTH_PCT) )); then
  echo "" >&2
  echo "REGRESSION: release WASM grew $((SIZE - BASELINE)) bytes ($(pct "$SIZE" "$BASELINE"))," >&2
  echo "  $BASELINE -> $SIZE bytes; allowed growth is +$GROWTH_PCT% (max $((BASELINE * (100 + GROWTH_PCT) / 100)) bytes)." >&2
  FAIL=1
fi
if (( SIZE > CEILING )); then
  echo "" >&2
  echo "CEILING: release WASM is $SIZE bytes, $((SIZE - CEILING)) bytes over the" >&2
  echo "  $CEILING-byte ceiling (network limit $LIMIT). Upgrades in place are at risk;" >&2
  echo "  see README.md 'deploy fresh instead of upgrade' and COST_MODEL.md §13." >&2
  FAIL=1
fi

if [[ "$EXPECT_FAIL" -eq 1 ]]; then
  if [[ "$FAIL" -eq 1 ]]; then
    echo "OK: size gate failed as expected (fixture)."
    exit 0
  fi
  echo "ERROR: expected the size gate to fail, but it passed." >&2
  exit 1
fi

if [[ "$FAIL" -eq 1 ]]; then
  echo "" >&2
  echo "If the growth is intended, run 'scripts/check_wasm_size.sh --update' and" >&2
  echo "commit wasm_size.toml, stating the reason in the PR (COST_MODEL.md §13)." >&2
  exit 1
fi
echo "OK: release WASM within growth budget and ceiling."
