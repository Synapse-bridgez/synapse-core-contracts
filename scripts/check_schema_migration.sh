#!/usr/bin/env bash
# Verify that schema_version() from the built release WASM has an exact
# matching entry in migrations.toml (#88).
#
# Usage:
#   scripts/check_schema_migration.sh [--manifest PATH] [--wasm PATH] [--expect-fail]
#
# Expects the release WASM to already exist (CI builds once, then reuses).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
MANIFEST="${ROOT}/migrations.toml"
WASM="${TARGET_DIR}/wasm32-unknown-unknown/release/synapse_core_contract.wasm"
EXPECT_FAIL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --manifest) MANIFEST="$2"; shift 2 ;;
    --wasm) WASM="$2"; shift 2 ;;
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

if [[ ! -f "$MANIFEST" ]]; then
  echo "ERROR: migrations manifest not found at: $MANIFEST" >&2
  exit 1
fi

export SYNAPSE_SCHEMA_CHECK_WASM="$WASM"
export SYNAPSE_SCHEMA_CHECK_MANIFEST="$MANIFEST"

set +e
# Filter to the wasm-vs-manifest test; the constant↔manifest test always runs.
cargo test --lib schema_version_from_release_wasm_matches_manifest -- --nocapture
STATUS=$?
set -e

if [[ "$EXPECT_FAIL" -eq 1 ]]; then
  if [[ "$STATUS" -ne 0 ]]; then
    echo "OK: schema check failed as expected (mismatch fixture)."
    exit 0
  fi
  echo "ERROR: expected schema check to fail against mismatched manifest, but it passed." >&2
  exit 1
fi

if [[ "$STATUS" -ne 0 ]]; then
  echo "" >&2
  echo "ERROR: schema_version() from the release WASM has no exact matching" >&2
  echo "entry in ${MANIFEST}." >&2
  echo "" >&2
  echo "Fix: add a [[migrations]] block with schema_version = <value> and a" >&2
  echo "human-readable note describing the migration, then re-run this check." >&2
  echo "See migrations.toml and DECISIONS.md / CHANGELOG.md." >&2
  exit 1
fi

echo "OK: schema_version() matches migrations.toml"
