#!/usr/bin/env bash
# diff_profiles.sh — differential test between the `release` and
# `release-with-logs` Cargo profiles (issue #131).
#
# Builds the test binary under each profile, records the behaviour trace
# produced by `test_profile_diff::profile_diff_emit_trace` (every entry
# point's return value, emitted events and resulting ledger state; see
# src/test_profile_diff.rs), and fails if the two traces differ.
#
# Usage:
#   scripts/diff_profiles.sh              # real comparison; exit 1 on divergence
#   scripts/diff_profiles.sh --self-test  # adds a deliberately divergent
#                                         # fixture; exit 1 if NOT detected
#
# Env:
#   TRACE_DIR  where to keep the trace files (default: a fresh temp dir)

set -euo pipefail

mode="compare"
case "${1:-}" in
  "") ;;
  --self-test) mode="self-test" ;;
  *) echo "usage: $0 [--self-test]" >&2; exit 2 ;;
esac

fixture=0
[[ "$mode" == "self-test" ]] && fixture=1

trace_dir="${TRACE_DIR:-$(mktemp -d)}"
mkdir -p "$trace_dir"

for profile in release release-with-logs; do
  out="$trace_dir/$profile.trace"
  rm -f "$out"
  echo "── $profile: recording behaviour trace → $out"
  SYNAPSE_PROFILE_TRACE_OUT="$out" SYNAPSE_PROFILE_DIFF_FIXTURE="$fixture" \
    cargo test --profile "$profile" --lib -- --exact \
      test_profile_diff::profile_diff_emit_trace
  if [[ ! -s "$out" ]]; then
    echo "error: $profile produced no trace (did the test filter match?)" >&2
    exit 1
  fi
done

a="$trace_dir/release.trace"
b="$trace_dir/release-with-logs.trace"
echo "── comparing $(grep -c '^#' "$a") recorded invocations"

if diff -u --label release "$a" --label release-with-logs "$b"; then
  if [[ "$mode" == "self-test" ]]; then
    echo "SELF-TEST FAILED: the injected debug_assertions divergence was not detected." >&2
    exit 1
  fi
  echo "OK: release and release-with-logs behave identically."
else
  if [[ "$mode" == "self-test" ]]; then
    echo "SELF-TEST OK: the injected divergence was detected (diff above is expected)."
    exit 0
  fi
  echo "DIVERGENCE: observable behaviour differs between release and release-with-logs." >&2
  echo "Something in the contract depends on debug-assertions; see the diff above." >&2
  exit 1
fi
