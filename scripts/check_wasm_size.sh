#!/usr/bin/env bash
# Release WASM binary-size gate (#122).
#
# Fails when the built WASM artifact is missing, when it grows more than
# `max_growth_pct` over the baseline in wasm_size.toml, or when it exceeds
# the hard `ceiling_bytes` cap.
#
# Usage:
#   scripts/check_wasm_size.sh            # check the current artifact
#   scripts/check_wasm_size.sh --update   # rewrite baseline_bytes in place

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE_FILE="$ROOT_DIR/wasm_size.toml"
ARTIFACT="$ROOT_DIR/target/wasm32-unknown-unknown/release/quickie.wasm"

if [[ ! -f "$ARTIFACT" ]]; then
  echo "error: WASM artifact not found: $ARTIFACT" >&2
  echo "       build it first with 'make wasm'" >&2
  exit 1
fi

if [[ ! -f "$BASELINE_FILE" ]]; then
  echo "error: baseline file not found: $BASELINE_FILE" >&2
  exit 1
fi

read_baseline() {
  local key="$1"
  grep -E "^[[:space:]]*${key}[[:space:]]*=" "$BASELINE_FILE" \
    | head -n1 \
    | sed -E 's/^[^=]*=[[:space:]]*//' \
    | tr -d '"'
}

baseline_bytes="$(read_baseline baseline_bytes)"
max_growth_pct="$(read_baseline max_growth_pct)"
ceiling_bytes="$(read_baseline ceiling_bytes)"

if [[ -z "$baseline_bytes" || -z "$max_growth_pct" || -z "$ceiling_bytes" ]]; then
  echo "error: could not parse baseline from $BASELINE_FILE" >&2
  exit 1
fi

actual_bytes="$(wc -c < "$ARTIFACT" | tr -d '[:space:]')"

if [[ "${1:-}" == "--update" ]]; then
  sed -i.bak -E "s/^[[:space:]]*baseline_bytes[[:space:]]*=.*/baseline_bytes = ${actual_bytes}/" "$BASELINE_FILE"
  rm -f "$BASELINE_FILE.bak"
  echo "updated baseline_bytes = ${actual_bytes} in $BASELINE_FILE"
  exit 0
fi

max_bytes=$(( baseline_bytes + baseline_bytes * max_growth_pct / 100 ))

printf 'wasm size: %s bytes (baseline %s, +%s%% => %s, ceiling %s)\n' \
  "$actual_bytes" "$baseline_bytes" "$max_growth_pct" "$max_bytes" "$ceiling_bytes"

status=0

if (( actual_bytes > max_bytes )); then
  echo "error: WASM grew beyond the ${max_growth_pct}% budget (${actual_bytes} > ${max_bytes})" >&2
  status=1
fi

if (( actual_bytes > ceiling_bytes )); then
  echo "error: WASM exceeds the hard ceiling (${actual_bytes} > ${ceiling_bytes})" >&2
  status=1
fi

if (( status != 0 )); then
  echo "       if the growth is intentional, run 'scripts/check_wasm_size.sh --update'" >&2
  exit "$status"
fi

echo "ok: WASM size within budget"
