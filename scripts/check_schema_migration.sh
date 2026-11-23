#!/usr/bin/env bash
# Verify that the release WASM's schema_version() has a matching entry in
# migrations.toml. See migrations.toml for the contract.
#
# Usage: scripts/check_schema_migration.sh [manifest-path]
#   Defaults to migrations.toml at the repo root.
#
# Exit codes:
#   0  manifest exists and contains an entry for the reported schema_version
#   1  manifest missing, unreadable, or missing the reported schema_version
#   2  usage / environment error (missing tooling, build failure, ...)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:-${REPO_ROOT}/migrations.toml}"

if [[ ! -f "${MANIFEST}" ]]; then
    echo "error: schema migration manifest not found: ${MANIFEST}" >&2
    echo "       expected a migrations.toml with [[migrations]] entries" >&2
    exit 1
fi

if [[ ! -r "${MANIFEST}" ]]; then
    echo "error: schema migration manifest is not readable: ${MANIFEST}" >&2
    exit 1
fi

# Build the release WASM and read the schema_version() it reports.
WASM="${REPO_ROOT}/target/wasm32-unknown-unknown/release/treasury.wasm"
if [[ ! -f "${WASM}" ]]; then
    echo "error: release WASM not found at ${WASM}; run 'make build' first" >&2
    exit 2
fi

REPORTED_VERSION="$(scripts/read_schema_version.sh "${WASM}")"
if [[ -z "${REPORTED_VERSION}" ]]; then
    echo "error: could not read schema_version() from ${WASM}" >&2
    exit 2
fi

# Extract every schema_version = N declared in the manifest.
DECLARED_VERSIONS="$(grep -E '^[[:space:]]*schema_version[[:space:]]*=' "${MANIFEST}" \
    | sed -E 's/^[[:space:]]*schema_version[[:space:]]*=[[:space:]]*//' \
    | tr -d '"' \
    | tr -d '[:space:]')"

if [[ -z "${DECLARED_VERSIONS}" ]]; then
    echo "error: no [[migrations]] schema_version entries found in ${MANIFEST}" >&2
    exit 1
fi

if ! grep -qx "${REPORTED_VERSION}" <<<"${DECLARED_VERSIONS}"; then
    echo "error: schema_version ${REPORTED_VERSION} reported by ${WASM}" >&2
    echo "       has no matching [[migrations]] entry in ${MANIFEST}" >&2
    echo "       declared versions: $(echo "${DECLARED_VERSIONS}" | tr '\n' ' ')" >&2
    exit 1
fi

echo "ok: schema_version ${REPORTED_VERSION} is documented in ${MANIFEST}"
