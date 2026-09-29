# ADR-0006 — Schema-version compatibility ranges

> **Status:** Accepted
> **Date:** 2026-Q3
> **Deciders:** Synapse Bridge core team, contract author
> **Issues:** #84
> **Amends:** [ADR-0003](./0003-upgrade-schema-version-guard.md)

---

## Context

ADR-0003's exact-match `expected_schema_version` guard is maximally safe
and maximally rigid. Additive, backward-compatible schema changes force
every upgrade call to be re-coordinated with an exact version number.

---

## Options considered

### Option A — Admin-declared `[min, max]` window ✅

- `set_schema_compatibility_range(min, max)` — admin-gated
- Must include the **current** on-chain schema version or reject with
  `InvalidSchemaCompatRange` (never create an un-upgradeable contract)
- `upgrade` / `propose_upgrade` / migrate / rollback path check
  `expected ∈ [min, max]`
- **Default when unset:** `min = max = schema_version()` — identical to
  ADR-0003 exact-match behaviour for existing deployments

### Option B — Infer compatibility from schema diffing

Out of scope (no static analysis on-chain).

---

## Decision

Adopt Option A as an explicit, opt-in weakening of ADR-0003 with a
default-safe fallback.

---

## Consequences

- **Positive:** Additive upgrades can proceed within an admin-declared window
  without weakening protection against out-of-range mismatches.
- **Negative:** A too-wide range is an operational foot-gun — mitigated by
  multisig admin requirements and monitoring of the setter (no dedicated
  event in v1; can be added later).

## References

- `src/lib.rs` — `set_schema_compatibility_range`, `schema_compatibility_range`
- ADR-0003
